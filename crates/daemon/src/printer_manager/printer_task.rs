use anyhow::{Result, anyhow};
use std::{sync::Arc, time::Instant};

use crate::{RUNTIME_CONFIG, lcd::LCDController, peripheral::PeripheralController};
use msla_core::{
    config,
    types::{
        model::{Model, ir::PrintingIR},
        peripheral::StepperPositioning,
        printer_manager::{PrinterState, PrinterTaskCommand, PrintingError, PrintingTaskMeta},
    },
};
use tokio::{
    sync::{
        mpsc::{Receiver, error::TryRecvError},
        oneshot,
        watch::Sender,
    },
    time::sleep,
};
use tracing::{debug, error};

/// Main printing task
pub struct PrinterTask {
    printing_model: Arc<Model>,
    current_layer: usize,
    current_ir_index: usize,
    current_ir_elapsed: Instant,
    total_elapsed: Instant,

    state: PrinterState,

    state_sender: Sender<PrinterState>,

    peripheral_controller: PeripheralController,
    lcd_controller: LCDController,

    z_level_offset: f64,

    /// Latest z position, before pausing (no z offset!)
    pause_position_context: f64,
}

impl PrinterTask {
    pub async fn new(
        state_sender: Sender<PrinterState>,
        printing_model: Arc<Model>,
        peripheral_controller: PeripheralController,
        lcd_controller: LCDController,
    ) -> Result<Self> {
        let z_offset = RUNTIME_CONFIG
            .config()?
            .read()
            .await
            .leveling
            .bed_level_offset;

        Ok(Self {
            state_sender,
            printing_model,
            peripheral_controller,
            lcd_controller,
            state: PrinterState::Idle,
            current_layer: 0,
            current_ir_index: 0,
            pause_position_context: 0.0,
            current_ir_elapsed: Instant::now(),
            total_elapsed: Instant::now(),
            z_level_offset: z_offset,
        })
    }

    pub async fn run(
        &mut self,
        mut command_receiver: Receiver<PrinterTaskCommand>,
        mut abort_oneshot: oneshot::Receiver<()>,
    ) {
        self.total_elapsed = Instant::now();
        self.state = PrinterState::Printing(self.build_task_meta());

        for i in 0..self.printing_model.ir.len() {
            self.current_ir_index = i;
            self.current_ir_elapsed = Instant::now();

            match command_receiver.try_recv() {
                Ok(PrinterTaskCommand::Pause) => {
                    self.state = PrinterState::Paused(self.build_task_meta());

                    if let Err(e) = self.abort_peripherals(true).await {
                        self.set_error_state(anyhow!("Cannot pause printer: {e}"));
                        self.emergency_stop().await;

                        return;
                    }

                    self.send_current_status();

                    tokio::select! {
                        resume = self.wait_to_resume(&mut command_receiver) => {
                            if let Err(e) = resume {
                                self.set_error_state(e);
                                self.emergency_stop().await;
                                return;
                            }

                            if let Err(e) = self.resume_peripherals().await {
                                self.set_error_state(e);
                                self.emergency_stop().await;
                                return;
                            }
                        }

                        _ = &mut abort_oneshot => {
                            self.abort().await;
                            return;
                        }
                    }
                },

                Err(TryRecvError::Empty) => {},
                Err(TryRecvError::Disconnected) => {
                    error!("Printer Task has lost external control!");
                    self.emergency_stop().await;

                    return;
                },

                _ => {},
            }

            let command = self.printing_model.ir[i].clone();
            self.state = PrinterState::Printing(self.build_task_meta());
            self.send_current_status();

            tokio::select! {
                result = self.execute_next_step(command.ir) => {
                    if let Err(e) = result {
                        self.set_error_state(e);
                        self.emergency_stop().await;

                        return;
                    }

                }

                _ = &mut abort_oneshot => {
                    self.abort().await;
                    return;
                }
            }
        }

        self.state = PrinterState::Finished {
            total_elapsed: self.total_elapsed,
        };
        self.send_current_status();
    }

    async fn execute_next_step(&mut self, command: PrintingIR) -> Result<(), PrintingError> {
        match command {
            PrintingIR::Home => {
                debug!("Homing Z...");
                self.peripheral_controller
                    .home_z(self.z_level_offset)
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot home Z axis: {e}")))?;

                self.pause_position_context = 0.0;
            },

            PrintingIR::MoveZ(mov) => {
                let mut pos = mov.pos;
                if matches!(mov.positioning, StepperPositioning::Absolute) {
                    pos += self.z_level_offset;
                }

                debug!("Move Z to {}mm, speed: {}mm/m", pos, mov.speed);

                self.peripheral_controller
                    .move_z_to(pos, mov.speed, mov.positioning)
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot move Z axis: {e}")))?;

                self.pause_position_context = mov.after_pos;
            },

            PrintingIR::TurnUV(state) => {
                debug!("Turn {} UV", if state { "on" } else { "off" });
                self.peripheral_controller
                    .turn_uv(state)
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot turn UV: {e}")))?;
            },

            PrintingIR::ShowImage(path_buf) => {
                self.lcd_controller
                    .show_image(self.printing_model.working_dir.path().join(path_buf))
                    .map_err(|e| PrintingError::new(format!("Cannot display image: {e}")))?;

                self.current_layer += 1;
            },

            PrintingIR::Wait(duration) => {
                debug!("Sleep {:?}", duration);
                sleep(duration).await;
            },

            PrintingIR::DisableSteppers => {
                debug!("Disable steppers");
                self.peripheral_controller
                    .disable_steppers()
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot disable steppers: {e}")))?;
            },

            PrintingIR::EnableSteppers => {
                debug!("Enable steppers");
                self.peripheral_controller
                    .enable_steppers()
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot enable steppers: {e}")))?;
            },

            PrintingIR::SetStepperCurrent(current) => {
                debug!("Set stepper current: {}mA", current);
                self.peripheral_controller
                    .set_motor_current(current)
                    .await
                    .map_err(|e| PrintingError::new(format!("Cannot set stepper current: {e}")))?;
            },
            PrintingIR::ForceErr(e) => {
                debug!("Called force error: {e}");
                return Err(PrintingError::new(e));
            },
        }

        Ok(())
    }

    /// Send status to printing manager
    fn send_current_status(&self) {
        self.state_sender.send_replace(self.state.clone());
    }

    /// Blocks current task until it's resumed or aborted
    ///
    /// Returns `false` if aborted, and `true` if resumed
    async fn wait_to_resume(
        &self,
        command_receiver: &mut Receiver<PrinterTaskCommand>,
    ) -> Result<()> {
        loop {
            match command_receiver.recv().await {
                Some(c) => match c {
                    PrinterTaskCommand::Pause => {},
                    PrinterTaskCommand::Resume => return Ok(()),
                },

                None => {
                    error!("Printer Task has lost external control!");
                    anyhow::bail!("External control lost");
                },
            }
        }
    }

    fn build_task_meta(&self) -> PrintingTaskMeta {
        PrintingTaskMeta {
            printing_layer: self.current_layer,
            current_ir_index: self.current_ir_index,
            current_ir_elapsed: self.current_ir_elapsed,
            total_elapsed: self.total_elapsed,
            model: self.printing_model.clone(),
        }
    }

    /// Sets error state for printer task
    fn set_error_state(&mut self, err: impl std::fmt::Display) {
        self.state = PrinterState::Error(PrintingError::new(err.to_string()));
        self.send_current_status();
    }

    /// Abort print and send aborted status
    async fn abort(&mut self) {
        self.state = PrinterState::Busy;
        self.send_current_status();

        match self.abort_peripherals(false).await {
            Ok(_) => self.state = PrinterState::Aborted,
            Err(e) => {
                self.state =
                    PrinterState::Error(PrintingError::new(format!("Cannot abort print: {e}")))
            },
        };

        self.send_current_status();
    }
}

impl PrinterTask {
    /// Stop peripherals
    async fn abort_peripherals(&mut self, paused: bool) -> Result<()> {
        let config = config::get_config().await;

        // Disable UV
        self.peripheral_controller.turn_uv(false).await?;

        // Tear off the printed material from the film
        self.peripheral_controller
            .move_z_to(5f64, 40f64, StepperPositioning::Relative)
            .await?;

        // Then quickly rise to the top
        self.peripheral_controller
            .move_z_to(
                config.physical.machine_height as f64,
                300f64,
                StepperPositioning::Absolute,
            )
            .await?;

        if !paused {
            // And... Disable stepper!
            self.peripheral_controller.disable_steppers().await?;
        }

        Ok(())
    }

    /// Emergency stop motor and disable uv
    async fn emergency_stop(&mut self) {
        error!("Emergency stop triggered!");

        if let Err(e) = self.peripheral_controller.turn_uv(false).await {
            error!("Failed to disable UV: {e}");
        }

        if let Err(e) = self.peripheral_controller.disable_steppers().await {
            error!("Failed to disable steppers: {e}");
        }
    }

    /// Return stepper to a latest pos
    async fn resume_peripherals(&mut self) -> Result<()> {
        self.peripheral_controller
            .move_z_to(
                self.pause_position_context + self.z_level_offset,
                50.0,
                StepperPositioning::Absolute,
            )
            .await
    }
}

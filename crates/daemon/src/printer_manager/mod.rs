pub mod printer_task;

use crate::{
    RUNTIME_CONFIG,
    lcd::LCDController,
    peripheral::PeripheralController,
    printer_manager::printer_task::PrinterTask,
};
use anyhow::Result;
use msla_core::types::{
    model::Model,
    peripheral::StepperPositioning,
    printer_manager::{PrinterCommand, PrinterState, PrinterTaskCommand, PrintingError},
};
use std::sync::Arc;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
    watch::{self, Receiver as ReceiverWatch, Sender as WatchSender},
};
use tracing::{error, info};

pub struct PrinterManager {
    /// Current printing state
    state: PrinterState,

    /// Command receiver for manager
    command_receiver: Receiver<PrinterCommand>,

    /// State transmitter for other control sources
    state_transmitter: WatchSender<PrinterState>,

    print_task_command_transmitter: Option<Sender<PrinterTaskCommand>>,
    print_task_state_receiver: Option<ReceiverWatch<PrinterState>>,

    print_task_aborter: Option<oneshot::Sender<()>>,

    peripheral_controller: PeripheralController,
    lcd_controller: LCDController,
}

impl PrinterManager {
    /// Create new printer instance
    pub fn new(
        receiver: Receiver<PrinterCommand>,
        transmitter: WatchSender<PrinterState>,
        per: PeripheralController,
        lcd: LCDController,
    ) -> Self {
        Self {
            state: PrinterState::Idle,
            command_receiver: receiver,
            state_transmitter: transmitter,
            peripheral_controller: per,
            lcd_controller: lcd,

            print_task_command_transmitter: None,
            print_task_state_receiver: None,
            print_task_aborter: None,
        }
    }

    /// Run printer manager instance
    pub async fn run(&mut self) -> Result<()> {
        info!("Started Printer Manager");
        info!("Waiting for incoming commands...");

        loop {
            tokio::select! {
                Some(command) = self.command_receiver.recv() => {
                    match command {
                        PrinterCommand::StartPrint(model) if !self.state.is_busy() => {
                            if let Err(e) = self.start_print(model).await {
                                error!("Cannot start print: {e}");
                                self.state = PrinterState::Error(PrintingError::new(e.to_string()))
                            }
                        },

                        PrinterCommand::Abort => {
                            if let Some(tx) = self.print_task_aborter.take() {
                                let _ = tx.send(());
                            }
                        },

                        PrinterCommand::Pause => {
                            self.send_to_print_task(PrinterTaskCommand::Pause).await;
                            if let PrinterState::Printing(st) = &self.state {
                                self.set_and_send_status(PrinterState::Pausing(st.clone()));
                            }
                        }

                        PrinterCommand::Resume => {
                            self.send_to_print_task(PrinterTaskCommand::Resume).await
                        }

                        PrinterCommand::Home if !self.state.is_busy() => {
                            let peripheral = self.peripheral_controller.clone();

                            self.run_busy(async move {
                                let z_offset = RUNTIME_CONFIG
                                    .config()?
                                    .read()
                                    .await
                                    .leveling
                                    .bed_level_offset;

                                peripheral.home_z(z_offset).await
                            }).await

                        },
                        PrinterCommand::DisableStepper if !self.state.is_busy() => {
                            let peripheral = self.peripheral_controller.clone();
                            self.run_busy(peripheral.disable_steppers()).await
                        },
                        PrinterCommand::MoveTo {pos, speed} if !self.state.is_busy() => {
                            let peripheral = self.peripheral_controller.clone();
                            self.run_busy(peripheral.move_z_to(pos, speed, StepperPositioning::Absolute)).await
                        }

                        _ => {}
                    }
                }

                result = async {
                    match &mut self.print_task_state_receiver {
                        Some(receiver) => receiver.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match result {
                        Ok(()) => {
                            let event = self
                                .print_task_state_receiver
                                .as_ref()
                                .unwrap()
                                .borrow()
                                .clone();

                            match event {
                                PrinterState::Aborted => {
                                    info!("Print aborted");
                                    self.reset_state().await;
                                }
                                PrinterState::Error(ref printing_error) => {
                                    error!("{}", printing_error);
                                    self.reset_state().await;
                                },

                                _ => {}

                            }

                            self.state = event;
                            self.send_status();
                        }

                        Err(_) => {
                            // PrinterTask destroyed Sender
                            self.print_task_state_receiver = None;
                        }
                    }
                }
            }
        }
    }

    /// Create Printer Task and start printing
    async fn start_print(&mut self, model: Arc<Model>) -> Result<()> {
        let (command_tx, command_rx) = mpsc::channel::<PrinterTaskCommand>(128);
        let (abort_tx, abort_rx) = oneshot::channel::<()>();
        let (state_tx, state_rx) = watch::channel(PrinterState::Idle);

        self.print_task_command_transmitter = Some(command_tx);
        self.print_task_state_receiver = Some(state_rx);
        self.print_task_aborter = Some(abort_tx);

        let per = self.peripheral_controller.clone();
        let lcd = self.lcd_controller.clone();

        let mut task = PrinterTask::new(state_tx, model, per, lcd).await?;
        tokio::spawn(async move {
            task.run(command_rx, abort_rx).await;
        });

        Ok(())
    }

    /// Clear all communications with print task
    async fn reset_state(&mut self) {
        self.peripheral_controller.reset_client().await;
        self.print_task_command_transmitter = None;
        self.print_task_state_receiver = None;
        self.print_task_aborter = None;
    }

    /// Send command into printing task
    async fn send_to_print_task(&mut self, task: PrinterTaskCommand) {
        if let Some(tx) = &self.print_task_command_transmitter {
            let _ = tx.send(task).await.map_err(|e| {
                error!("Cannot send to a print task: {e}");
                self.state =
                    PrinterState::Error(PrintingError::new("Cannot send to a print task: {e}"))
            });
        }
    }

    /// Send current status to all external listeners
    fn send_status(&mut self) {
        self.state_transmitter.send_replace(self.state.clone());
    }

    /// Sets current status and send to a channel
    fn set_and_send_status(&mut self, status: PrinterState) {
        self.state = status;
        self.send_status();
    }

    /// Runs async task and sets Busy state during exec.
    async fn run_busy<F>(&mut self, op: F)
    where
        F: Future<Output = Result<()>>,
    {
        self.set_and_send_status(PrinterState::Busy);

        let state = match op.await {
            Ok(_) => PrinterState::Idle,
            Err(e) => {
                error!("Printer op failed: {e}");
                PrinterState::Error(PrintingError::new(e.to_string()))
            },
        };

        self.set_and_send_status(state);
    }
}

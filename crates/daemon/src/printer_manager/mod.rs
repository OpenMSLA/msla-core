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
    printer_manager::{PrinterCommand, PrinterState, PrintingError},
};
use std::sync::Arc;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
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

    print_task_command_transmitter: Option<Sender<PrinterCommand>>,
    print_task_state_receiver: Option<ReceiverWatch<PrinterState>>,

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
                        PrinterCommand::StartPrint(model) => {
                            if let Err(e) = self.start_print(model).await {
                                error!("Cannot start print: {e}");
                                self.state = PrinterState::Error(PrintingError::new(e.to_string()))
                            }
                        },
                        PrinterCommand::Abort |
                        PrinterCommand::Pause |
                        PrinterCommand::Resume =>
                            self.send_to_print_task(command).await,

                        PrinterCommand::Home => {
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
                        PrinterCommand::DisableStepper => {
                            let peripheral = self.peripheral_controller.clone();
                            self.run_busy(async move {
                                peripheral.disable_steppers().await
                            }).await
                        },
                        PrinterCommand::MoveTo {pos, speed} => {
                            let peripheral = self.peripheral_controller.clone();
                            self.run_busy(async move {
                                peripheral.move_z_to(pos, speed, StepperPositioning::Absolute).await
                            }).await
                        }
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
                            self.send_status().await;
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
        let (command_tx, command_rx) = mpsc::channel::<PrinterCommand>(128);
        let (state_tx, state_rx) = watch::channel(PrinterState::Idle);

        self.print_task_command_transmitter = Some(command_tx);
        self.print_task_state_receiver = Some(state_rx);

        let per = self.peripheral_controller.clone();
        let lcd = self.lcd_controller.clone();

        let mut task = PrinterTask::new(state_tx, model, per, lcd).await?;
        tokio::spawn(async move {
            task.run(command_rx).await;
        });

        Ok(())
    }

    /// Clear all communications with print task
    async fn reset_state(&mut self) {
        self.peripheral_controller.reset_client().await;
        self.print_task_command_transmitter = None;
        self.print_task_state_receiver = None;
    }

    /// Send command into printing task
    async fn send_to_print_task(&mut self, task: PrinterCommand) {
        if let Some(tx) = &self.print_task_command_transmitter {
            let _ = tx.send(task).await.map_err(|e| {
                error!("Cannot send to a print task: {e}");
                self.state =
                    PrinterState::Error(PrintingError::new("Cannot send to a print task: {e}"))
            });
        }
    }

    /// Send current status to all external listeners
    async fn send_status(&mut self) {
        let _ = self.state_transmitter.send_replace(self.state.clone());
    }

    /// Sets current status and send to a channel
    async fn set_and_send_status(&mut self, status: PrinterState) {
        self.state = status;
        self.send_status().await;
    }

    /// Runs async task and sets Busy state during exec.
    async fn run_busy<F>(&mut self, op: F)
    where
        F: Future<Output = Result<()>>,
    {
        self.set_and_send_status(PrinterState::Busy).await;

        let state = match op.await {
            Ok(_) => PrinterState::Idle,
            Err(e) => {
                error!("Printer op failed: {e}");
                PrinterState::Error(PrintingError::new(e.to_string()))
            },
        };

        self.set_and_send_status(state).await;
    }
}

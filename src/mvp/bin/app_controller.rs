use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::controller::{Controller, ControllerChannels, ControllerMessages};
use crate::error::Error;
use crate::model::{Model, ModelChannels, ModelMessages};
use crate::ui::{Ui, UiChannels, UiMessages};

#[derive(Debug)]
struct AppControllerChannels {
    receiver: tokio::sync::mpsc::Receiver<AppControllerMessages>,
}

#[derive(Debug)]
pub enum AppControllerMessages {
    UiRequestsQuit,
}

#[derive(Debug, PartialEq)]
enum AppControllerState {
    New,
    Running,
}

pub struct AppController {
    state: Arc<Mutex<AppControllerState>>,
    waker: Option<Waker>,
}

impl AppController {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            state: Arc::new(Mutex::new(AppControllerState::New)),
            waker: None,
        })
    }

    fn spawn_app_controller(
        mut receiver: tokio::sync::mpsc::Receiver<AppControllerMessages>,
        _to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
    ) -> Result<tokio::task::JoinHandle<Result<(), Error>>, Error> {
        Ok(tokio::spawn(async move {
            match receiver.recv().await {
                Some(AppControllerMessages::UiRequestsQuit) => {
                    receiver.close();
                }
                None => {
                    panic!("AppController receiver received None");
                }
            }
            Ok(())
        }))
    }

    fn spawn_controller(
        receiver: tokio::sync::mpsc::Receiver<ControllerMessages>,
        to_app_controller: tokio::sync::mpsc::Sender<AppControllerMessages>,
        to_model: crossbeam_channel::Sender<ModelMessages>,
        to_ui: tokio::sync::mpsc::Sender<UiMessages>,
    ) -> Result<tokio::task::JoinHandle<Result<(), Error>>, Error> {
        let controller = Controller::new(ControllerChannels {
            receiver,
            to_app_controller,
            to_model,
            to_ui,
        })?;
        Ok(tokio::spawn(async move { controller.await }))
    }

    fn spawn_model(
        receiver: crossbeam_channel::Receiver<ModelMessages>,
        to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
    ) -> Result<tokio::task::JoinHandle<Result<(), Error>>, Error> {
        let model = Model::new(ModelChannels {
            receiver,
            to_controller,
        })?;
        Ok(tokio::spawn(async move { model.await }))
    }

    fn spawn_ui(
        receiver: tokio::sync::mpsc::Receiver<UiMessages>,
        to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
    ) -> Result<tokio::task::JoinHandle<Result<(), Error>>, Error> {
        let ui = Ui::new(UiChannels {
            receiver,
            to_controller,
        })?;
        Ok(tokio::spawn(async move { ui.await }))
    }

    fn wait_for_tasks(
        app_controller_task: tokio::task::JoinHandle<Result<(), Error>>,
        controller_task: tokio::task::JoinHandle<Result<(), Error>>,
        model_task: tokio::task::JoinHandle<Result<(), Error>>,
        ui_task: tokio::task::JoinHandle<Result<(), Error>>,
        waker: Waker,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let (app_controller_result, controller_result, model_result, ui_result) =
                tokio::join!(app_controller_task, controller_task, model_task, ui_task,);
            app_controller_result
                .expect("AppController task panicked")
                .expect("AppController task failed");
            controller_result
                .expect("Controller task panicked")
                .expect("Controller task failed");
            model_result
                .expect("Model task panicked")
                .expect("Model task failed");
            ui_result
                .expect("Ui task panicked")
                .expect("Ui task failed");
            waker.wake();
        })
    }
}

impl Future for AppController {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        {
            self.waker = Some(context.waker().clone());
        }
        {
            let mut state_guard = self.state.lock().unwrap();
            if *state_guard == AppControllerState::New {
                // Build channels and objects
                let (app_controller_sender, app_controller_receiver) =
                    tokio::sync::mpsc::channel(1);
                let (controller_sender, controller_receiver) = tokio::sync::mpsc::channel(1);
                let (model_sender, model_receiver) = crossbeam_channel::bounded(1);
                let (ui_sender, ui_receiver) = tokio::sync::mpsc::channel(1);
                let app_controller_task = AppController::spawn_app_controller(
                    app_controller_receiver,
                    controller_sender.clone(),
                )
                .expect("Failed to spawn App Controller");
                let controller_task = AppController::spawn_controller(
                    controller_receiver,
                    app_controller_sender,
                    model_sender,
                    ui_sender,
                )
                .expect("Failed to spawn Controller");
                let model_task =
                    AppController::spawn_model(model_receiver, controller_sender.clone())
                        .expect("Failed to spawn Model");
                let ui_task = AppController::spawn_ui(ui_receiver, controller_sender)
                    .expect("Failed to spawn Ui");

                *state_guard = AppControllerState::Running;

                AppController::wait_for_tasks(
                    app_controller_task,
                    controller_task,
                    model_task,
                    ui_task,
                    context.waker().clone(),
                );

                return Poll::Pending;
            }
        }
        Poll::Ready(Ok(()))
    }
}

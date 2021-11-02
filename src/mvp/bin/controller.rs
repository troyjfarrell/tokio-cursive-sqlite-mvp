use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::app_controller::AppControllerMessages;
use crate::error::Error;
use crate::model::{ModelMessages, ModelValues};
use crate::ui::UiMessages;

#[derive(Debug)]
pub struct ControllerChannels {
    pub receiver: tokio::sync::mpsc::Receiver<ControllerMessages>,
    pub to_app_controller: tokio::sync::mpsc::Sender<AppControllerMessages>,
    pub to_model: crossbeam_channel::Sender<ModelMessages>,
    pub to_ui: tokio::sync::mpsc::Sender<UiMessages>,
}

#[derive(Debug)]
pub enum ControllerMessages {
    GetModelSchemaVersion(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    GetPersons(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateDown(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateToLatest(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateUp(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    UiRequestsQuit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControllerState {
    New,
    Running,
    Finished,
}

pub struct Controller {
    channels: Option<Arc<Mutex<ControllerChannels>>>,
    state: Arc<Mutex<ControllerState>>,
    state_receiver: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<ControllerState>>>>,
    waker: Option<Waker>,
}

impl Controller {
    pub fn new(channels: ControllerChannels) -> Result<Self, Error> {
        Ok(Self {
            channels: Some(Arc::new(Mutex::new(channels))),
            state: Arc::new(Mutex::new(ControllerState::New)),
            state_receiver: Arc::new(Mutex::new(None)),
            waker: None,
        })
    }

    fn run(
        controller_channels: Arc<Mutex<ControllerChannels>>,
        state_sender: tokio::sync::mpsc::Sender<ControllerState>,
        waker: Waker,
    ) -> tokio::task::JoinHandle<()> {
        let channels_mutex =
            Arc::<Mutex<ControllerChannels>>::try_unwrap(controller_channels).unwrap();
        let ControllerChannels {
            mut receiver,
            to_app_controller,
            to_model,
            to_ui,
        } = channels_mutex.into_inner().unwrap();
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Some(ControllerMessages::GetModelSchemaVersion(response_channel)) => {
                        to_model
                            .send(ModelMessages::GetModelSchemaVersion(response_channel))
                            .expect("Failed to send GetModelSchemaVersion to Model");
                    }
                    Some(ControllerMessages::GetPersons(response_channel)) => {
                        to_model
                            .send(ModelMessages::GetPersons(response_channel))
                            .expect("Failed to send GetPersons to Model");
                    }
                    Some(ControllerMessages::MigrateDown(response_channel)) => {
                        to_model
                            .send(ModelMessages::MigrateDown(response_channel))
                            .expect("Failed to send MigrateDown to Model");
                    }
                    Some(ControllerMessages::MigrateToLatest(response_channel)) => {
                        to_model
                            .send(ModelMessages::MigrateToLatest(response_channel))
                            .expect("Failed to send MigrateToLatest to Model");
                    }
                    Some(ControllerMessages::MigrateUp(response_channel)) => {
                        to_model
                            .send(ModelMessages::MigrateUp(response_channel))
                            .expect("Failed to send MigrateUp to Model");
                    }
                    Some(ControllerMessages::UiRequestsQuit) => {
                        to_model
                            .send(ModelMessages::UiRequestsQuit)
                            .expect("Failed to send UiRquestsQuit to Model");
                        to_ui
                            .send(UiMessages::UiRequestsQuit)
                            .await
                            .expect("Failed to send UiRequestsQuit to Ui");
                        to_app_controller
                            .send(AppControllerMessages::UiRequestsQuit)
                            .await
                            .expect("Failed to send UiRequestsQuit to AppController");
                        state_sender
                            .send(ControllerState::Finished)
                            .await
                            .expect("Failed to send Finished to Controller");
                        waker.wake();
                        break;
                    }
                    None => {
                        break;
                    }
                }
            }
        })
    }
}

impl Future for Controller {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        {
            self.waker = Some(context.waker().clone());
        }
        let maybe_channels: Option<Arc<Mutex<ControllerChannels>>> = self.channels.clone();
        {
            if self.channels.is_some() {
                self.channels = None;
            }
        }
        {
            let mut state_guard = self.state.lock().unwrap();
            let mut state_receiver_guard = self.state_receiver.lock().unwrap();
            if *state_guard == ControllerState::New {
                match maybe_channels {
                    Some(controller_channels) => {
                        let (tx, rx) = tokio::sync::mpsc::channel(1);
                        *state_receiver_guard = Some(rx);
                        Self::run(controller_channels, tx, context.waker().clone());
                        *state_guard = ControllerState::Running;
                    }
                    _ => {
                        panic!("Cannot run controller task without ControllerChannels");
                    }
                }
            } else {
                // mut deref kung fu
                if (*state_receiver_guard).is_some() {
                    match (*state_receiver_guard).as_mut().unwrap().poll_recv(context) {
                        Poll::Pending => {}
                        Poll::Ready(Some(ControllerState::New)) => {
                            return Poll::Ready(Err(Error::InvalidControllerStateTransition(
                                state_guard.clone(),
                                ControllerState::New,
                            )));
                        }
                        Poll::Ready(Some(next_state)) => {
                            *state_guard = next_state;
                        }
                        Poll::Ready(None) => {
                            return Poll::Ready(Err(Error::ControllerStateSenderClosed));
                        }
                    }
                } else {
                    return Poll::Ready(Err(Error::ControllerMissingStateReceiver));
                }
            }
            if *state_guard == ControllerState::Finished {
                return Poll::Ready(Ok(()));
            }
        }
        Poll::Pending
    }
}

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use cursive::views::{Button, Dialog, LinearLayout, NamedView, TextView};

use crate::controller::ControllerMessages;
use crate::error::Error;
use crate::model::{ModelValues, Person, SchemaVersion, SchemaVersionInfo};

#[derive(Debug)]
pub struct UiChannels {
    pub receiver: tokio::sync::mpsc::Receiver<UiMessages>,
    pub to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
}

#[derive(Debug)]
pub enum UiMessages {
    UiRequestsQuit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiState {
    New,
    Running,
    Finished,
}

pub struct Ui {
    channels: Option<Arc<Mutex<UiChannels>>>,
    state: Arc<Mutex<UiState>>,
    state_receiver: Option<Arc<Mutex<tokio::sync::mpsc::Receiver<UiState>>>>,
    thread_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    waker: Option<Waker>,
}

impl Ui {
    pub fn new(channels: UiChannels) -> Result<Self, Error> {
        Ok(Self {
            channels: Some(Arc::new(Mutex::new(channels))),
            state: Arc::new(Mutex::new(UiState::New)),
            state_receiver: None,
            thread_handle: Arc::new(Mutex::new(None)),
            waker: None,
        })
    }

    fn add_global_callbacks(
        cur: &mut cursive::Cursive,
        to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
    ) {
        cur.add_global_callback('q', move |_| {
            Ui::quit_callback(&mut to_controller.clone());
        });
    }

    fn first_screen(
        cur: &mut cursive::Cursive,
        to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        use cursive_core::view::Nameable;

        // Clone army
        let to_controller1 = to_controller.clone();
        let to_controller2 = to_controller.clone();
        let to_controller3 = to_controller.clone();
        let to_controller4 = to_controller.clone();
        let responses_sender1 = responses_sender.clone();
        let responses_sender2 = responses_sender.clone();
        let responses_sender3 = responses_sender.clone();

        cur.add_layer(
            LinearLayout::horizontal()
                .child(TextView::new("Current Schema Version: 0").with_name("schema_version"))
                .child(
                    LinearLayout::vertical()
                        .child(NamedView::new(
                            "migrate_down_button",
                            Button::new("Migrate down", move |_| {
                                Ui::migrate_down(
                                    &mut to_controller1.clone(),
                                    responses_sender1.clone(),
                                );
                            }),
                        ))
                        .child(NamedView::new(
                            "migrate_to_latest_button",
                            Button::new("Migrate to latest", move |_| {
                                Ui::migrate_to_latest(
                                    &mut to_controller2.clone(),
                                    responses_sender2.clone(),
                                );
                            }),
                        ))
                        .child(NamedView::new(
                            "migrate_up_button",
                            Button::new("Migrate up", move |_| {
                                Ui::migrate_up(
                                    &mut to_controller3.clone(),
                                    responses_sender3.clone(),
                                );
                            }),
                        ))
                        .child(NamedView::new(
                            "show_persons_button",
                            Button::new("Show Persons", move |_| {
                                Ui::get_persons(
                                    &mut to_controller4.clone(),
                                    responses_sender.clone(),
                                );
                            }),
                        ))
                        .child(NamedView::new(
                            "quit_button",
                            Button::new("Quit", move |_| {
                                Ui::quit_callback(&mut to_controller.clone());
                            }),
                        )),
                ),
        );
    }

    fn get_model_schema_version(
        tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx_to_controller
            .blocking_send(ControllerMessages::GetModelSchemaVersion(tx))
            .expect("Failed to send GetModelSchemaVersion to Controller");
        responses_sender
            .blocking_send(rx)
            .expect("Failed to send the GetModelSchemaVersion oneshot receiver");
    }

    fn get_persons(
        tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx_to_controller
            .blocking_send(ControllerMessages::GetPersons(tx))
            .expect("Failed to send GetPersons to Controller");
        responses_sender
            .blocking_send(rx)
            .expect("Failed to send the GetPersons oneshot receiver");
    }

    fn handle_messages<T>(
        cur: &mut cursive::Cursive,
        uimessages_receiver: &mut tokio::sync::mpsc::Receiver<UiMessages>,
        responses_receiver: &mut tokio::sync::mpsc::Receiver<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
        response_oneshots: &mut Vec<tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>>,
        state_sender: tokio::sync::mpsc::Sender<UiState>,
        waker: std::task::Waker,
        finish_after: std::time::Instant,
        wait_time: std::time::Duration,
    ) -> Result<(), Error> {
        use retain_mut::RetainMut;

        loop {
            match uimessages_receiver.try_recv() {
                Ok(UiMessages::UiRequestsQuit) => {
                    cur.quit();
                    state_sender
                        .blocking_send(UiState::Finished)
                        .expect("Failed to send Finished to Ui (1)");
                    waker.wake();
                    return Ok(());
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    error!("Ui receiver has become disconnected.");
                    state_sender
                        .blocking_send(UiState::Finished)
                        .expect("Failed to send Finished to Ui (2)");
                    // It's an error, but the result is the same: program exit
                    return Ok(());
                }
            }
            match responses_receiver.try_recv() {
                Ok(oneshot) => {
                    response_oneshots.push(oneshot);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    error!("Ui responses_receiver disconnected");
                }
            }
            response_oneshots.retain_mut(|oneshot| match oneshot.try_recv() {
                Ok(response) => {
                    match response {
                        Ok(ModelValues::Persons(persons)) => {
                            Ui::handle_persons(cur, &persons);
                        }
                        Ok(ModelValues::SchemaVersionInfo(schema_version_info)) => {
                            Ui::handle_schema_version_info(cur, &schema_version_info);
                        }
                        Err(error) => {
                            error!("Message reponse oneshot error: {}", error);
                        }
                    };
                    false
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => true,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    error!("Response oneshot was closed");
                    false
                }
            });
            let now = std::time::Instant::now();
            if now > finish_after {
                return Ok(());
            }
            std::thread::sleep(wait_time);
        }
    }

    fn handle_persons(cur: &mut cursive::Cursive, persons: &[Person]) {
        let person_string = persons
            .iter()
            .map(|person| person.format_row())
            .collect::<Vec<String>>()
            .join("\n");
        cur.add_layer(
            Dialog::around(TextView::new(person_string))
                .title("Persons")
                .button("OK", |siv| {
                    siv.pop_layer();
                }),
        );
    }

    fn handle_schema_version_info(
        cur: &mut cursive::Cursive,
        schema_version_info: &SchemaVersionInfo,
    ) {
        let version = match schema_version_info.current_version {
            SchemaVersion::NoneSet => {
                cur.call_on_name("migrate_down_button", |button: &mut Button| {
                    button.disable();
                });
                cur.call_on_name("migrate_to_latest_button", |button: &mut Button| {
                    button.enable();
                });
                cur.call_on_name("migrate_up_button", |button: &mut Button| {
                    button.enable();
                });
                cur.call_on_name("show_persons_button", |button: &mut Button| {
                    button.disable();
                });
                cur.focus_name("migrate_to_latest_button")
                    .expect("migrate_to_latest_button view not found");
                "0".to_string()
            }
            SchemaVersion::Expected(v) => {
                let max_version = schema_version_info.max_version.get();
                cur.call_on_name("migrate_down_button", |button: &mut Button| {
                    button.set_enabled(v > 0);
                });
                cur.call_on_name("migrate_to_latest_button", |button: &mut Button| {
                    button.set_enabled(v < max_version);
                });
                cur.call_on_name("migrate_up_button", |button: &mut Button| {
                    button.set_enabled(v < max_version);
                });
                cur.call_on_name("show_persons_button", |button: &mut Button| {
                    button.enable();
                });
                if v == max_version {
                    cur.focus_name("migrate_down_button")
                        .expect("migrate_down_button view not found");
                } else if v == 0 {
                    cur.focus_name("migrate_to_latest_button")
                        .expect("migrate_to_latest_button view not found");
                }
                v.to_string()
            }
            SchemaVersion::Unexpected(v) => {
                cur.call_on_name("migrate_down_button", |button: &mut Button| {
                    button.disable();
                });
                cur.call_on_name("migrate_to_latest_button", |button: &mut Button| {
                    button.disable();
                });
                cur.call_on_name("migrate_up_button", |button: &mut Button| {
                    button.disable();
                });
                cur.focus_name("quit_button")
                    .expect("quit_button view not found");
                v.to_string()
            }
        };
        cur.call_on_name("schema_version", move |view: &mut TextView| {
            view.set_content(format!("Current Version: {}", version));
        });
    }

    fn migrate_down(
        tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx_to_controller
            .blocking_send(ControllerMessages::MigrateDown(tx))
            .expect("Failed to send MigrateDown to Controller");
        responses_sender
            .blocking_send(rx)
            .expect("Failed to send the MigrateDown oneshot receiver");
    }

    fn migrate_to_latest(
        tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx_to_controller
            .blocking_send(ControllerMessages::MigrateToLatest(tx))
            .expect("Failed to send MigrateToLatest to Controller");
        responses_sender
            .blocking_send(rx)
            .expect("Failed to send the MigrateToLatest oneshot receiver");
    }

    fn migrate_up(
        tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>,
        responses_sender: tokio::sync::mpsc::Sender<
            tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
        >,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx_to_controller
            .blocking_send(ControllerMessages::MigrateUp(tx))
            .expect("Failed to send MigrateUp to Controller");
        responses_sender
            .blocking_send(rx)
            .expect("Failed to send the MigrateUp oneshot receiver");
    }

    fn quit_callback(tx_to_controller: &mut tokio::sync::mpsc::Sender<ControllerMessages>) {
        tx_to_controller
            .blocking_send(ControllerMessages::UiRequestsQuit)
            .expect("Failed to send UiRequestsQuit to Controller");
    }

    fn run(
        ui_channels: Arc<Mutex<UiChannels>>,
        state_sender: tokio::sync::mpsc::Sender<UiState>,
        waker: std::task::Waker,
    ) -> std::thread::JoinHandle<()> {
        let channels_mutex = Arc::<Mutex<UiChannels>>::try_unwrap(ui_channels).unwrap();
        let UiChannels {
            receiver: mut uimessages_receiver,
            mut to_controller,
        } = channels_mutex.into_inner().unwrap();

        std::thread::spawn(move || {
            let mut cur = cursive::default();
            let (responses_sender, mut responses_receiver) = tokio::sync::mpsc::channel(1);
            let mut response_oneshots: Vec<
                tokio::sync::oneshot::Receiver<Result<ModelValues, Error>>,
            > = Vec::new();

            Ui::add_global_callbacks(&mut cur, to_controller.clone());
            Ui::first_screen(&mut cur, to_controller.clone(), responses_sender.clone());
            Ui::get_model_schema_version(&mut to_controller, responses_sender);

            // Getting started
            let mut runner = cur.into_runner();
            let frame = std::time::Duration::from_millis(33);
            let mut last_frame = std::time::Instant::now();
            let no_wait = std::time::Duration::new(0, 0);
            let mut refresh_this_iteration = true;
            let mut timeout: std::time::Duration = frame;
            {
                while runner.is_running() {
                    let something_happened = runner.process_events();
                    if something_happened || refresh_this_iteration {
                        if refresh_this_iteration {
                            runner.on_event(cursive_core::event::Event::Refresh);
                        }
                        runner.refresh();
                    }
                    if something_happened {
                        timeout = no_wait;
                    }
                    let next_frame = last_frame + timeout;
                    if Ui::handle_messages::<cursive::Cursive>(
                        &mut runner,
                        &mut uimessages_receiver,
                        &mut responses_receiver,
                        &mut response_oneshots,
                        state_sender.clone(),
                        waker.clone(),
                        next_frame,
                        frame,
                    )
                    .is_err()
                    {
                        runner.quit();
                        return;
                    }
                    let now = std::time::Instant::now();
                    if now >= next_frame {
                        last_frame = now;
                        refresh_this_iteration = true;
                        timeout = frame;
                    } else {
                        refresh_this_iteration = false;
                        timeout = next_frame - now;
                    }
                }
            }
        })
    }
}

impl Future for Ui {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        {
            self.waker = Some(context.waker().clone());
        }
        let maybe_channels: Option<Arc<Mutex<UiChannels>>> = self.channels.clone();
        {
            if self.channels.is_some() {
                self.channels = None;
            }
        }
        let maybe_state_receiver: Option<Arc<Mutex<tokio::sync::mpsc::Receiver<UiState>>>> =
            self.state_receiver.clone();
        let maybe_future_state_receiver: Option<Arc<Mutex<tokio::sync::mpsc::Receiver<UiState>>>>;
        {
            if self.state_receiver.is_some() {
                self.state_receiver = None;
            }
        }
        {
            let mut state_guard = self.state.lock().unwrap();
            let mut thread_handle_guard = self.thread_handle.lock().unwrap();
            if *state_guard == UiState::New {
                match maybe_channels {
                    Some(ui_channels) => {
                        let (tx, rx) = tokio::sync::mpsc::channel(1);
                        maybe_future_state_receiver = Some(Arc::new(Mutex::new(rx)));
                        *thread_handle_guard =
                            Some(Self::run(ui_channels, tx, context.waker().clone()));

                        *state_guard = UiState::Running;
                    }
                    _ => {
                        panic!("Cannot run ui task without UiChannels");
                    }
                }
            } else if maybe_state_receiver.is_some() {
                let arc_mutex_receiver = maybe_state_receiver.unwrap();
                let mutex_receiver =
                    Arc::<Mutex<tokio::sync::mpsc::Receiver<UiState>>>::try_unwrap(
                        arc_mutex_receiver,
                    )
                    .unwrap();
                let mut receiver = mutex_receiver.into_inner().unwrap();
                let state_change = receiver.try_recv();
                match state_change {
                    Ok(UiState::New) => {
                        return Poll::Ready(Err(Error::InvalidUiStateTransition(
                            state_guard.clone(),
                            UiState::New,
                        )));
                    }
                    Ok(new_state) => {
                        *state_guard = new_state;
                    }
                    _ => {}
                };
                maybe_future_state_receiver = Some(Arc::new(Mutex::new(receiver)));
            } else {
                return Poll::Ready(Err(Error::UiMissingStateReceiver));
            }
            if *state_guard == UiState::Finished {
                return Poll::Ready(Ok(()));
            }
        }
        // Put the StateReceiver back.
        if self.state_receiver.is_none() && maybe_future_state_receiver.is_some() {
            self.state_receiver = maybe_future_state_receiver;
        } else {
            return Poll::Ready(Err(Error::UiWouldClobberStateReceiver));
        }
        Poll::Pending
    }
}

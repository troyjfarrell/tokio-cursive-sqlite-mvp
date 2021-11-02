mod migrations;
mod person;

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use rusqlite_migration::SchemaVersion as SqliteSchemaVersion;

use crate::controller::ControllerMessages;
use crate::error::Error;

pub use migrations::{SchemaVersion, SchemaVersionInfo};
pub use person::Person;

const DATABASE: &str = "mvp.sqlite";

const SQL_SELECT_FROM_PERSONS_V1: &str = "SELECT id, name, age FROM persons";
const SQL_SELECT_FROM_PERSONS_V2: &str = "SELECT id, name, age, state FROM persons";

#[derive(Debug)]
pub struct ModelChannels {
    pub receiver: crossbeam_channel::Receiver<ModelMessages>,
    pub to_controller: tokio::sync::mpsc::Sender<ControllerMessages>,
}

#[derive(Debug)]
pub enum ModelMessages {
    GetModelSchemaVersion(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    GetPersons(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateDown(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateToLatest(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    MigrateUp(tokio::sync::oneshot::Sender<Result<ModelValues, Error>>),
    UiRequestsQuit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelState {
    New,
    Running,
    Finished,
}

pub struct Model {
    channels: Option<Arc<Mutex<ModelChannels>>>,
    state: Arc<Mutex<ModelState>>,
    state_receiver: Arc<Mutex<Option<crossbeam_channel::Receiver<ModelState>>>>,
    thread_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    waker: Option<Waker>,
}

impl Model {
    pub fn new(channels: ModelChannels) -> Result<Self, Error> {
        Ok(Self {
            channels: Some(Arc::new(Mutex::new(channels))),
            state: Arc::new(Mutex::new(ModelState::New)),
            state_receiver: Arc::new(Mutex::new(None)),
            thread_handle: Arc::new(Mutex::new(None)),
            waker: None,
        })
    }

    fn run(
        model_channels: Arc<Mutex<ModelChannels>>,
        state_sender: crossbeam_channel::Sender<ModelState>,
        waker: Waker,
    ) -> std::thread::JoinHandle<()> {
        let channels_mutex = Arc::<Mutex<ModelChannels>>::try_unwrap(model_channels).unwrap();
        let ModelChannels {
            receiver,
            to_controller: _to_controller,
        } = channels_mutex.into_inner().unwrap();

        std::thread::spawn(move || {
            let mut connection =
                rusqlite::Connection::open(DATABASE).expect("Failed to open SQLite database");
            loop {
                match receiver.recv() {
                    Ok(message) => match message {
                        ModelMessages::GetModelSchemaVersion(responder) => {
                            let version = migrations::MIGRATIONS
                                .current_version(&connection)
                                .expect("Failed to get current schema version");
                            responder
                                .send(Ok(version.into()))
                                .expect("Failed to send GetModelSchemaVersion response");
                        }
                        ModelMessages::GetPersons(responder) => {
                            let mut has_table_persons = false;
                            let mut has_state_column = false;
                            connection
                                .pragma(None, "table_info", &"persons", |row| {
                                    has_table_persons = true;
                                    if row.get("name").unwrap_or_else(|_| "".to_string()) == "state"
                                    {
                                        has_state_column = true;
                                    }
                                    Ok(())
                                })
                                .expect("Querying SQLite table_info failed");
                            if !has_table_persons {
                                responder
                                    .send(Err(Error::ModelSchemaValuesNotAvailable))
                                    .expect("Failed to send GetPersons error response");
                            } else {
                                let sql = match has_state_column {
                                    true => SQL_SELECT_FROM_PERSONS_V2,
                                    false => SQL_SELECT_FROM_PERSONS_V1,
                                };
                                let mut get_persons = connection
                                    .prepare(sql)
                                    .expect("Failed to prepare SELECT FROM persons");
                                let persons: Vec<Person> = get_persons
                                    .query_map([], |row| {
                                        Ok(Person {
                                            id: row.get("id").expect("Failed to get persons.id"),
                                            name: row
                                                .get("name")
                                                .expect("Failed to get persons.name"),
                                            age: row.get("age").expect("Failed to get persons.age"),
                                            state: row
                                                .get("state")
                                                .unwrap_or_else(|_| "".to_string()),
                                        })
                                    })
                                    .expect("Failed to query persons table")
                                    .map(|result| result.expect("Failed to make a Person struct"))
                                    .collect();
                                responder
                                    .send(Ok(ModelValues::Persons(persons)))
                                    .expect("Failed to send GetPersons response");
                            }
                        }
                        ModelMessages::MigrateDown(responder) => {
                            let version = migrations::MIGRATIONS
                                .current_version(&connection)
                                .expect("Failed to get current model schema version");
                            let one = NonZeroUsize::new(1).unwrap();
                            //                            if let SqliteSchemaVersion::Inside(v) = version && v > one {
                            if let SqliteSchemaVersion::Inside(v) = version {
                                if v >= one {
                                    let new_version: usize = v.get().checked_sub(1).unwrap();
                                    match migrations::MIGRATIONS
                                        .to_version(&mut connection, new_version)
                                    {
                                        Ok(()) => {
                                            let now_version = migrations::MIGRATIONS.current_version(&connection).expect("Failed to get model schema version after migration");
                                            responder.send(Ok(now_version.into())).expect(
                                                "Failed to send MigrateDown success response",
                                            );
                                        }
                                        Err(error) => {
                                            responder
                                                .send(Err(Error::ModelSchemaMigrateDownFailed(
                                                    error,
                                                )))
                                                .expect(
                                                    "Failed to send MigrateDown error response (1)",
                                                );
                                        }
                                    }
                                }
                                // This else will be unnecessary when Rust supports
                                // if let ... && ...
                                else {
                                    responder
                                        .send(Err(Error::ModelSchemaMigrateDownNotAvailable))
                                        .expect("Failed to send MigrateDown error response (2)");
                                }
                            } else {
                                responder
                                    .send(Err(Error::ModelSchemaMigrateDownNotAvailable))
                                    .expect("Failed to send MigrateDown error response (3)");
                            }
                        }
                        ModelMessages::MigrateToLatest(responder) => {
                            match migrations::MIGRATIONS.to_latest(&mut connection) {
                                Ok(()) => {
                                    let now_version =
                                        migrations::MIGRATIONS.current_version(&connection).expect(
                                            "Failed to get model schema version after to_latest",
                                        );
                                    responder
                                        .send(Ok(now_version.into()))
                                        .expect("Failed to send MigrateToLatest success response");
                                }
                                Err(error) => {
                                    responder
                                        .send(Err(Error::ModelSchemaMigrateToLatestFailed(error)))
                                        .expect("Failed to send MigrateToLatest error response");
                                }
                            }
                        }
                        ModelMessages::MigrateUp(responder) => {
                            let version = migrations::MIGRATIONS
                                .current_version(&connection)
                                .expect("Failed to get current model schema version");
                            match version {
                                SqliteSchemaVersion::NoneSet => {
                                    if migrations::MAX_VERSION >= 1 {
                                        let new_version: usize = 1;
                                        match migrations::MIGRATIONS
                                            .to_version(&mut connection, new_version)
                                        {
                                            Ok(()) => {
                                                let now_version = migrations::MIGRATIONS.current_version(&connection).expect("Failed to get model schema version after migration");
                                                responder.send(Ok(now_version.into())).expect(
                                                    "Failed to send MigrateUp success response",
                                                );
                                            }
                                            Err(error) => {
                                                responder
                                                    .send(Err(Error::ModelSchemaMigrateUpFailed(error)))
                                                .expect("Failed to send MigrateUp error response (1)");
                                            }
                                        }
                                    } else {
                                        responder
                                            .send(Err(Error::ModelSchemaMigrateUpNotAvailable))
                                            .expect("Failed to send MigrateUp error response (2)");
                                    }
                                }
                                SqliteSchemaVersion::Inside(v) => {
                                    let max = NonZeroUsize::new(migrations::MAX_VERSION).unwrap();
                                    if v >= max {
                                        responder
                                            .send(Err(Error::ModelSchemaMigrateUpNotAvailable))
                                            .expect("Failed to send MigrateUp error response (3)");
                                    } else {
                                        let new_version: usize = v.get().checked_add(1).unwrap();
                                        match migrations::MIGRATIONS
                                            .to_version(&mut connection, new_version)
                                        {
                                            Ok(()) => {
                                                let now_version = migrations::MIGRATIONS.current_version(&connection).expect("Failed to get model schema version after migration");
                                                responder.send(Ok(now_version.into())).expect(
                                                    "Failed to send MigrateUp success response",
                                                );
                                            }
                                            Err(error) => {
                                                responder
                                                    .send(Err(Error::ModelSchemaMigrateUpFailed(error)))
                                                .expect("Failed to send MigrateUp error response (4)");
                                            }
                                        }
                                    }
                                }
                                SqliteSchemaVersion::Outside(_) => {
                                    responder
                                        .send(Err(Error::ModelSchemaMigrateUpNotAvailable))
                                        .expect("Failed to send MigrateUp error response (5)");
                                }
                            }
                        }
                        ModelMessages::UiRequestsQuit => {
                            state_sender
                                .send(ModelState::Finished)
                                .expect("Failed to send Finished to Model (1)");
                            waker.wake();
                            return;
                        }
                    },
                    // Err means that there will be no more messages
                    Err(_) => {
                        state_sender
                            .send(ModelState::Finished)
                            .expect("Failed to send Finished to Model (2)");
                        waker.wake();
                        return;
                    }
                }
            }
        })
    }
}

impl Future for Model {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        {
            self.waker = Some(context.waker().clone());
        }
        let maybe_channels: Option<Arc<Mutex<ModelChannels>>> = self.channels.clone();
        {
            if self.channels.is_some() {
                self.channels = None;
            }
        }
        {
            let mut state_guard = self.state.lock().unwrap();
            let mut state_receiver_guard = self.state_receiver.lock().unwrap();
            let mut thread_handle_guard = self.thread_handle.lock().unwrap();

            if *state_guard == ModelState::New {
                match maybe_channels {
                    Some(model_channels) => {
                        let (tx, rx) = crossbeam_channel::bounded(1);
                        *state_receiver_guard = Some(rx);
                        *thread_handle_guard =
                            Some(Self::run(model_channels, tx, context.waker().clone()));

                        *state_guard = ModelState::Running;
                    }
                    _ => {
                        panic!("Cannot run model task without ModelChannels");
                    }
                }
            } else {
                match &*state_receiver_guard {
                    Some(receiver) => {
                        let state_change = receiver.try_recv();
                        match state_change {
                            Ok(ModelState::New) => {
                                return Poll::Ready(Err(Error::InvalidModelStateTransition(
                                    state_guard.clone(),
                                    ModelState::New,
                                )));
                            }
                            Ok(new_state) => {
                                *state_guard = new_state;
                            }
                            _ => {}
                        };
                    }
                    None => {
                        return Poll::Ready(Err(Error::ModelMissingStateReceiver));
                    }
                }
            }
            if *state_guard == ModelState::Finished {
                return Poll::Ready(Ok(()));
            }
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub enum ModelValues {
    Persons(Vec<Person>),
    SchemaVersionInfo(SchemaVersionInfo),
}

impl From<SqliteSchemaVersion> for ModelValues {
    fn from(schema_version: SqliteSchemaVersion) -> Self {
        Self::SchemaVersionInfo(SchemaVersionInfo::from_sqlite_schema_version(
            schema_version,
        ))
    }
}

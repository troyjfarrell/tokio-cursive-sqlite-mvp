# Minimum Viable Program Using Tokio, Cursive and SQLite

This program demonstrates the use of the Tokio asynchronous framework, the
Cursive text-based user interface and SQLite from Rust.  The user interface and
SQLite libraries run on their own threads and communicate with a Tokio-based
controller task via Tokio channels and Crossbeam channels.

## Architecture

The MVP creates the `AppController` object.  The `AppController` implements the
`Future` trait, so the Tokio runtime simply `.await`s the `AppController` to
run the AppController's `poll` method.  The `poll` method creates the
`Controller`, `Model` and `Ui` objects and `.await`s them in Tokio tasks.

The `Model` and `Ui` tasks spawn separate threads, as the Cursive TUI and
Rusqlite libraries are not easily fit into Tokio's `Send + Sync` world.
Instead, these threads run loops of their own which communicate with the Tokio
tasks through Tokio and Crossbeam channels.

## Author's Note

I am new to Rust and inexperienced in writing desktop applications.  Feedback
on how to improve the architecture is welcome.

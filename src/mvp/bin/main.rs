mod app_controller;
mod controller;
mod error;
mod model;
mod ui;

#[macro_use]
extern crate log;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use app_controller::AppController;

    pretty_env_logger::init();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app_controller = AppController::new().unwrap();
        app_controller.await.unwrap();
    });
    Ok(())
}

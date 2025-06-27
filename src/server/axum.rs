use axum::*;
use dioxus_fullstack::prelude::*;

pub async fn serve() {
    // Get the address the server should run on. If the CLI is running, the CLI proxies fullstack into the main address
    // and we use the generated address the CLI gives us
    let address = dioxus_cli_config::fullstack_address_or_localhost();

    // Set up the axum router
    let router = Router::new()
        // You can add a dioxus application to the router with the `serve_dioxus_application` method
        // This will add a fallback route to the router that will serve your component and server functions
        .serve_dioxus_application(ServeConfig::new().unwrap(), App);

    // Finally, we can launch the server
    let router = router.into_make_service();
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

fn app() -> Element {
    rsx! { "Hello World" }
}
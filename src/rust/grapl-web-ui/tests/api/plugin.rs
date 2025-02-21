#![cfg(feature = "integration_tests")]

use grapl_web_ui::routes::api::plugin::{
    create::CreateResponse,
    // get_analyzers::GetAnalyzersResponse,
    get_deployment::PluginDeploymentResponse,
    get_health::GetPluginHealthResponse,
    get_metadata::GetPluginMetadataResponse,
};
use rust_proto::graplinc::grapl::api::plugin_registry::v1beta1::{
    PluginDeploymentStatus,
    PluginHealthStatus,
};

use crate::test_app::TestApp;

#[actix_web::test]
async fn plugin_lifecycle() -> eyre::Result<()> {
    let app = TestApp::init().await?;

    app.login_with_test_user().await?;

    let plugin_name = "Test Plugin";

    //TODO: re-enable when we upload analyzers
    // let analyzers = get_analyzers(&app).await?.plugin_ids;
    // eyre::ensure!(
    //     analyzers.is_empty(),
    //     "expected to analyzers at this point, but found {}",
    //     analyzers.len()
    // );

    let create_response = create_plugin(&app, plugin_name).await?;

    //TODO: this shouldn't be necessary, but we're seeing 50 errors without it.
    // I'll file a task to look into it for now so we can unblock frontend work.
    std::thread::sleep(std::time::Duration::from_secs(5));

    let plugin_metadata = get_plugin_metadata(&app, &create_response.plugin_id).await?;

    eyre::ensure!(
        create_response.plugin_id == plugin_metadata.plugin_id,
        "unexpected plugin_id: {:?}",
        plugin_metadata.plugin_id
    );

    let plugin_id = create_response.plugin_id;

    eyre::ensure!(
        plugin_name == plugin_metadata.display_name,
        "get_plugin_metadata returned unexpected name: {}, should be {plugin_name}",
        plugin_metadata.display_name
    );

    // verify plugin is not deployed yet
    let plugin_health = get_health(&app, &plugin_id).await?;
    eyre::ensure!(
        plugin_health.health_status == PluginHealthStatus::NotDeployed,
        "plugin health expected to be 'not_deployed'"
    );

    //TODO: this shouldn't be necessary, but we're seeing 50 errors without it.
    // I'll file a task to look into it for now so we can unblock frontend work.
    std::thread::sleep(std::time::Duration::from_secs(5));

    deploy_plugin(&app, &plugin_id).await?;

    let deployment_status = get_deployment(&app, &plugin_id).await?;

    eyre::ensure!(deployment_status.deployed, "plugin not deployed :(");

    eyre::ensure!(
        deployment_status.status == PluginDeploymentStatus::Success,
        "plugin deployment not successful"
    );

    // check health again, ensure it is running
    let plugin_health = get_health(&app, &plugin_id).await?;
    eyre::ensure!(
        plugin_health.health_status == PluginHealthStatus::Running,
        "plugin health expected to be 'running'"
    );

    // cool, now tear it down
    tear_down(&app, &plugin_id).await?;

    // get new status
    let deployment_status = get_deployment(&app, &plugin_id).await?;

    eyre::ensure!(
        !deployment_status.deployed,
        "plugin still deployed after tear_down :("
    );

    //check health again, ensure dead
    let plugin_health = get_health(&app, &plugin_id).await?;
    eyre::ensure!(
        plugin_health.health_status == PluginHealthStatus::Dead,
        "plugin health expected to be 'dead'"
    );

    Ok(())
}

async fn create_plugin(app: &TestApp, plugin_name: &str) -> eyre::Result<CreateResponse> {
    let create_metadata_body = serde_json::json!({
            "plugin_name": plugin_name,
            "plugin_type": "generator",
            "event_source_id": uuid::Uuid::new_v4()
    });

    let generator_bytes = e2e_tests::test_fixtures::get_sysmon_generator()?;

    let form = reqwest::multipart::Form::new()
        .part(
            "metadata",
            reqwest::multipart::Part::text(create_metadata_body.to_string()),
        )
        .part(
            "plugin_artifact",
            reqwest::multipart::Part::bytes(generator_bytes.to_vec()),
        );

    let response = app.post("api/plugin/create").multipart(form).send().await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    let response_body = response.json::<CreateResponse>().await?;

    println!("create response body: {:?}", response_body);

    Ok(response_body)
}

async fn get_plugin_metadata(
    app: &TestApp,
    plugin_id: &uuid::Uuid,
) -> eyre::Result<GetPluginMetadataResponse> {
    let response = app
        .get(format!("api/plugin/get_metadata?plugin_id={plugin_id}").as_str())
        .send()
        .await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    let response_body = response.json::<GetPluginMetadataResponse>().await?;

    println!("metadata response body: {:?}", response_body);

    Ok(response_body)
}

async fn deploy_plugin(app: &TestApp, plugin_id: &uuid::Uuid) -> eyre::Result<()> {
    let body = serde_json::json!({
            "plugin_id": plugin_id,
    });

    let response = app.post("api/plugin/deploy").json(&body).send().await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    Ok(())
}

async fn get_deployment(
    app: &TestApp,
    plugin_id: &uuid::Uuid,
) -> eyre::Result<PluginDeploymentResponse> {
    let response = app
        .get(format!("api/plugin/get_deployment?plugin_id={plugin_id}").as_str())
        .send()
        .await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    let response_body = response.json::<PluginDeploymentResponse>().await?;

    println!("response body: {:?}", response_body);

    Ok(response_body)
}

async fn tear_down(app: &TestApp, plugin_id: &uuid::Uuid) -> eyre::Result<()> {
    let body = serde_json::json!({
            "plugin_id": plugin_id,
    });

    let response = app.post("api/plugin/tear_down").json(&body).send().await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    Ok(())
}

async fn get_health(
    app: &TestApp,
    plugin_id: &uuid::Uuid,
) -> eyre::Result<GetPluginHealthResponse> {
    let response = app
        .get(format!("api/plugin/get_health?plugin_id={plugin_id}").as_str())
        .send()
        .await?;

    eyre::ensure!(
        response.status() == actix_web::http::StatusCode::OK,
        "unexpected response: {:?}",
        &response
    );

    let response_body = response.json::<GetPluginHealthResponse>().await?;

    println!("response body: {:?}", response_body);

    Ok(response_body)
}

// async fn get_analyzers(app: &TestApp) -> eyre::Result<GetAnalyzersResponse> {
//     let response = app.get("api/plugin/get_analyzers").send().await?;

//     eyre::ensure!(
//         response.status() == actix_web::http::StatusCode::OK,
//         "unexpected response: {:?}",
//         &response
//     );

//     let response_body = response.json::<GetAnalyzersResponse>().await?;

//     println!("response body: {:?}", response_body);

//     Ok(response_body)
// }

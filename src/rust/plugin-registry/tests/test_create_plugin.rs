#![cfg(feature = "integration_tests")]

use bytes::Bytes;
use clap::Parser;
use grapl_utils::future_ext::GraplFutureExt;
use rust_proto::{
    client_factory::services::PluginRegistryClientConfig,
    graplinc::grapl::api::plugin_registry::v1beta1::{
        GetPluginRequest,
        GetPluginResponse,
        PluginMetadata,
        PluginRegistryServiceClient,
        PluginType,
    },
    protocol::service_client::ConnectWithConfig,
};

/// For now, this is just a smoke test. This test can and should evolve as
/// the service matures.
#[test_log::test(tokio::test)]
async fn test_create_plugin() -> eyre::Result<()> {
    tracing::debug!(
        env=?std::env::args(),
    );
    let client_config = PluginRegistryClientConfig::parse();
    let mut client = PluginRegistryServiceClient::connect_with_config(client_config).await?;

    let tenant_id = uuid::Uuid::new_v4();

    let display_name = uuid::Uuid::new_v4().to_string();

    let event_source_id = uuid::Uuid::new_v4();

    let meta = PluginMetadata::new(
        tenant_id,
        display_name.clone(),
        PluginType::Generator,
        Some(event_source_id),
    );

    let single_chunk = Bytes::from("dummy vec for now");

    let response = client
        .create_plugin(
            meta,
            futures::stream::once(async move { single_chunk.clone() }),
        )
        .timeout(std::time::Duration::from_secs(5))
        .await??;

    let plugin_id = response.plugin_id();

    let get_response: GetPluginResponse = client
        .get_plugin(GetPluginRequest::new(plugin_id, tenant_id))
        .timeout(std::time::Duration::from_secs(5))
        .await??;
    assert_eq!(get_response.plugin_id(), plugin_id);
    assert_eq!(
        get_response.plugin_metadata().plugin_type(),
        PluginType::Generator
    );
    assert_eq!(get_response.plugin_metadata().display_name(), &display_name);
    assert_eq!(
        get_response.plugin_metadata().event_source_id(),
        Some(event_source_id)
    );

    Ok(())
}

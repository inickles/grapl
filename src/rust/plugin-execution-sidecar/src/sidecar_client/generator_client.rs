use rust_proto::{
    client_factory::services::GeneratorClientConfig,
    graplinc::grapl::api::plugin_sdk::generators::v1beta1::client::GeneratorServiceClient,
    protocol::service_client::{
        ConnectError,
        ConnectWithConfig,
    },
};

fn get_plugin_upstream_address(plugin_id: uuid::Uuid) -> String {
    let upstream_addr_env_var = format!("NOMAD_UPSTREAM_ADDR_plugin-{plugin_id}");
    let upstream_addr = std::env::var(&upstream_addr_env_var).expect(&upstream_addr_env_var);
    let address = format!("http://{upstream_addr}");
    address
}

/// Create a client from environment
pub async fn get_generator_client(
    plugin_id: uuid::Uuid,
) -> Result<GeneratorServiceClient, ConnectError> {
    let address = get_plugin_upstream_address(plugin_id);
    let client_config = GeneratorClientConfig {
        generator_client_address: address.parse().expect("generator_client_address"),
    };
    GeneratorServiceClient::connect_with_config(client_config).await
}

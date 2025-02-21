use clap::Parser;
use generator_dispatcher::{
    config::GeneratorDispatcherConfig,
    GeneratorDispatcher,
};
use rust_proto::{
    client_factory::services::PluginWorkQueueClientConfig,
    graplinc::grapl::api::plugin_work_queue::v1beta1::PluginWorkQueueServiceClient,
    protocol::service_client::ConnectWithConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = grapl_tracing::setup_tracing("generator-dispatcher")?;
    let config = GeneratorDispatcherConfig::parse();
    let plugin_work_queue_client_config = PluginWorkQueueClientConfig::parse();
    let plugin_work_queue_client =
        PluginWorkQueueServiceClient::connect_with_config(plugin_work_queue_client_config).await?;
    let worker_pool_size = config.params.worker_pool_size;
    let mut generator_dispatcher =
        GeneratorDispatcher::new(config, plugin_work_queue_client).await?;

    generator_dispatcher.run(worker_pool_size).await?;

    Ok(())
}

use std::time::Duration;

use async_cache::{
    AsyncCache,
    AsyncCacheError,
};
use clap::Parser;
use futures::{
    pin_mut,
    StreamExt,
    TryStreamExt,
};
use kafka::{
    config::{
        ConsumerConfig,
        RetryProducerConfig,
    },
    CommitError,
    ConfigurationError as KafkaConfigurationError,
    Consumer,
    ConsumerError,
    ProducerError,
    RetryProducer,
};
use rust_proto::{
    client_factory::services::{
        PluginRegistryClientConfig,
        PluginWorkQueueClientConfig,
    },
    graplinc::grapl::{
        api::{
            graph::v1beta1::MergedGraph,
            plugin_registry::v1beta1::{
                GetAnalyzersForTenantRequest,
                PluginRegistryServiceClient,
            },
            plugin_work_queue::v1beta1::{
                ExecutionJob,
                PluginWorkQueueServiceClient,
                PluginWorkQueueServiceClientError,
                PushExecuteAnalyzerRequest,
            },
        },
        pipeline::v1beta1::Envelope,
    },
    protocol::{
        error::GrpcClientError,
        service_client::{
            ConnectError,
            ConnectWithConfig,
        },
        status::{
            Code,
            Status,
        },
    },
    SerDe,
    SerDeError,
};
use thiserror::Error;
use tracing::Instrument;
use uuid::Uuid;

#[derive(clap::Parser, Clone, Debug)]
struct AnalyzerDispatcherConfigParams {
    #[clap(long, env = "WORKER_POOL_SIZE")]
    pub worker_pool_size: usize,

    #[clap(long, env = "ANALYZER_IDS_CACHE_CAPACITY")]
    pub analyzer_ids_cache_capacity: u64,

    #[clap(long, env = "ANALYZER_IDS_CACHE_TTL_MS")]
    pub analyzer_ids_cache_ttl_ms: u64,

    #[clap(long, env = "ANALYZER_IDS_CACHE_UPDATER_POOL_SIZE")]
    pub analyzer_ids_cache_updater_pool_size: usize,

    #[clap(long, env = "ANALYZER_IDS_CACHE_UPDATER_QUEUE_DEPTH")]
    pub analyzer_ids_cache_updater_queue_depth: usize,
}

#[derive(Clone, Debug)]
struct AnalyzerDispatcherConfig {
    pub kafka_config: ConsumerConfig,
    pub kafka_retry_producer_config: RetryProducerConfig,
    pub params: AnalyzerDispatcherConfigParams,
}

impl AnalyzerDispatcherConfig {
    pub fn parse() -> Self {
        Self {
            kafka_config: ConsumerConfig::parse(),
            kafka_retry_producer_config: RetryProducerConfig::parse(),
            params: AnalyzerDispatcherConfigParams::parse(),
        }
    }
}

#[derive(Debug, Error)]
enum ConfigurationError {
    #[error("error configuring kafka client")]
    KafkaConfiguration(#[from] KafkaConfigurationError),

    #[error("error configuring analyzer IDs cache")]
    AnalyzerIdsCacheConfiguration(#[from] ConnectError),
}

#[derive(Debug, Error)]
enum AnalyzerDispatcherError {
    #[error("analyzer IDs cache error {0}")]
    AnalyzerIdsCache(#[from] AsyncCacheError),

    // FIXME: don't crash the service when this happens
    #[error("error sending data to plugin-work-queue {0}")]
    PluginWorkQueueClient(#[from] PluginWorkQueueServiceClientError),

    // FIXME: also don't crash the service when this happens
    #[error("error trying to send message to kafka {0}")]
    Producer(#[from] ProducerError),

    #[error("error committing kafka consumer offsets {0}")]
    Commit(#[from] CommitError),

    #[error("error serializing or deserializing protobuf data {0}")]
    SerDe(#[from] SerDeError),
}

struct AnalyzerDispatcher {
    plugin_work_queue_client: PluginWorkQueueServiceClient,
    merged_graphs_consumer: Consumer<MergedGraph>,
    merged_graphs_retry_producer: RetryProducer<MergedGraph>,
    analyzer_ids_cache: AsyncCache<Uuid, Vec<Uuid>>,
}

impl AnalyzerDispatcher {
    pub async fn new(
        config: AnalyzerDispatcherConfig,
        plugin_work_queue_client: PluginWorkQueueServiceClient,
    ) -> Result<Self, ConfigurationError> {
        let merged_graphs_consumer: Consumer<MergedGraph> = Consumer::new(config.kafka_config)?;
        let merged_graphs_retry_producer: RetryProducer<MergedGraph> =
            RetryProducer::new(config.kafka_retry_producer_config)?;
        let client_config = PluginRegistryClientConfig::parse();

        let plugin_registry_client =
            PluginRegistryServiceClient::connect_with_config(client_config).await?;

        let analyzer_ids_cache = AsyncCache::new(
            config.params.analyzer_ids_cache_capacity,
            Duration::from_millis(config.params.analyzer_ids_cache_ttl_ms),
            config.params.analyzer_ids_cache_updater_pool_size,
            config.params.analyzer_ids_cache_updater_queue_depth,
            move |tenant_id| {
                let mut plugin_registry_client = plugin_registry_client.clone();

                async move {
                    match plugin_registry_client
                        .get_analyzers_for_tenant(GetAnalyzersForTenantRequest::new(tenant_id))
                        .await
                    {
                        Ok(response) => Some(response.plugin_ids().to_vec()),
                        Err(GrpcClientError::ErrorStatus(Status {
                            code: Code::NotFound,
                            ..
                        })) => {
                            tracing::warn!(
                                message = "found no analyzers for tenant",
                                tenant_id =% tenant_id,
                            );
                            Some(vec![])
                        }
                        Err(e) => {
                            // failed to update the cache, but the message will
                            // be retried via the kafka retry topic
                            tracing::error!(
                                message = "error retrieving analyzers for tenant",
                                tenant_id =% tenant_id,
                                reason =% e,
                            );

                            None
                        }
                    }
                }
            },
        )
        .await;

        Ok(Self {
            plugin_work_queue_client,
            merged_graphs_consumer,
            merged_graphs_retry_producer,
            analyzer_ids_cache,
        })
    }

    pub async fn run(&mut self, pool_size: usize) -> Result<(), AnalyzerDispatcherError> {
        let analyzer_ids_cache = self.analyzer_ids_cache.clone();
        let plugin_work_queue_client = self.plugin_work_queue_client.clone();
        let merged_graphs_retry_producer = self.merged_graphs_retry_producer.clone();

        loop {
            let analyzer_ids_cache = analyzer_ids_cache.clone();
            let plugin_work_queue_client = plugin_work_queue_client.clone();
            let merged_graphs_retry_producer = merged_graphs_retry_producer.clone();

            let stream = self.merged_graphs_consumer
                .stream()
                .take(pool_size)
                .then(move |merged_graphs_result: Result<(tracing::Span, Envelope<MergedGraph>), ConsumerError>| {
                    let analyzer_ids_cache = analyzer_ids_cache.clone();
                    let plugin_work_queue_client = plugin_work_queue_client.clone();
                    let merged_graphs_retry_producer = merged_graphs_retry_producer.clone();

                    async move {
                        match merged_graphs_result {
                            Ok((span, envelope)) => {
                                match analyzer_ids_cache
                                    .clone()
                                    .get(envelope.tenant_id())
                                    .instrument(span.clone())
                                    .await
                                {
                                    Ok(Some(analyzer_ids)) => {
                                        if analyzer_ids.is_empty() {
                                            let _guard = span.enter();
                                            tracing::warn!(
                                                message = "no analyzers for tenant",
                                            );
                                        } else {
                                            enqueue_plugin_work(
                                                plugin_work_queue_client.clone(),
                                                analyzer_ids,
                                                envelope,
                                            )
                                                .instrument(span)
                                                .await?;
                                        }

                                        Ok(())
                                    },
                                    Ok(None) => {
                                        // cache miss, but an update was
                                        // successfully enqueued so we'll retry
                                        // the message
                                        let _guard = span.enter();
                                        tracing::debug!(
                                            message = "analyzer IDs cache miss, retrying message",
                                        );
                                        drop(_guard);

                                        retry_message(
                                            &merged_graphs_retry_producer,
                                            envelope,
                                        )
                                            .instrument(span)
                                            .await?;

                                        Ok(())
                                    },
                                    Err(AsyncCacheError::Retryable(reason)) => {
                                        // retryable cache error, so we'll retry
                                        // the message
                                        let _guard = span.enter();
                                        tracing::warn!(
                                            message = "analyzer IDs cache error, retrying message",
                                            reason =% reason,
                                        );
                                        drop(_guard);

                                        retry_message(
                                            &merged_graphs_retry_producer,
                                            envelope,
                                        )
                                            .instrument(span)
                                            .await?;

                                        Ok(())
                                    },
                                    Err(cache_err) => {
                                        // fatal error, bailing out
                                        Err(AnalyzerDispatcherError::from(cache_err))
                                    }
                                }
                            },
                            Err(e) => {
                                tracing::error!(
                                    message = "error processing kafka message",
                                    reason =% e,
                                );

                                Ok(())
                            },
                        }
                    }
                })
                .then(|result| async {
                    self.merged_graphs_consumer.commit()?;
                    result
                });

            pin_mut!(stream);

            while let Some(result) = stream.next().await {
                if let Err(e) = result {
                    tracing::error!(
                        message = "fatal error",
                        reason =% e,
                    );
                    return Err(e);
                }
            }
        }
    }
}

async fn retry_message(
    merged_graphs_retry_producer: &RetryProducer<MergedGraph>,
    envelope: Envelope<MergedGraph>,
) -> Result<(), ProducerError> {
    // TODO: be a little smarter about handling ProducerError here
    merged_graphs_retry_producer.send(envelope).await
}

#[tracing::instrument(skip(plugin_work_queue_client, analyzer_ids, envelope), err)]
async fn enqueue_plugin_work(
    plugin_work_queue_client: PluginWorkQueueServiceClient,
    analyzer_ids: Vec<Uuid>,
    envelope: Envelope<MergedGraph>,
) -> Result<(), AnalyzerDispatcherError> {
    let pool_size = analyzer_ids.len();
    let tenant_id = envelope.tenant_id();
    let trace_id = envelope.trace_id();
    let event_source_id = envelope.event_source_id();
    let payload = envelope.inner_message().serialize()?;
    futures::stream::iter(analyzer_ids)
        .map(|analyzer_id| Ok(analyzer_id))
        .try_for_each_concurrent(pool_size, move |analyzer_id| {
            let payload = payload.clone();
            let mut plugin_work_queue_client = plugin_work_queue_client.clone();

            async move {
                let execution_job =
                    ExecutionJob::new(payload.clone(), tenant_id, trace_id, event_source_id);

                tracing::debug!(
                    message = "enqueueing analyzer execution job",
                    analyzer_id =% analyzer_id,
                );

                // TODO: retries, backpressure signalling, etc.
                plugin_work_queue_client
                    .push_execute_analyzer(PushExecuteAnalyzerRequest::new(
                        execution_job,
                        analyzer_id,
                    ))
                    .await
                    .map(|_| ())
                    .map_err(|e| AnalyzerDispatcherError::from(e))
            }
        })
        .await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = grapl_tracing::setup_tracing("analyzer-dispatcher");
    let config = AnalyzerDispatcherConfig::parse();
    let plugin_work_queue_client_config = PluginWorkQueueClientConfig::parse();
    let plugin_work_queue_client =
        PluginWorkQueueServiceClient::connect_with_config(plugin_work_queue_client_config).await?;
    let worker_pool_size = config.params.worker_pool_size;
    let mut analyzer_dispatcher = AnalyzerDispatcher::new(config, plugin_work_queue_client).await?;

    analyzer_dispatcher.run(worker_pool_size).await?;

    Ok(())
}

#![recursion_limit = "1024"]

use clap::Parser;
use futures::{
    FutureExt,
    StreamExt,
};
use grapl_config::env_helpers::FromEnv;
use grapl_tracing::setup_tracing;
use kafka::{
    config::{
        ConsumerConfig,
        ProducerConfig,
    },
    StreamProcessor,
    StreamProcessorError,
};
use rusoto_dynamodb::DynamoDbClient;
use rust_proto::graplinc::grapl::{
    api::graph::v1beta1::{
        GraphDescription,
        IdentifiedGraph,
    },
    pipeline::v1beta1::Envelope,
};
use tracing::{
    instrument::WithSubscriber,
    Instrument,
};

mod dynamic_sessiondb;
mod error;
mod node_identifier;
mod sessiondb;
mod sessions;

use crate::{
    dynamic_sessiondb::NodeDescriptionIdentifier,
    error::NodeIdentifierError,
    node_identifier::NodeIdentifier,
    sessiondb::SessionDb,
};

const SERVICE_NAME: &'static str = "node-identifier";

#[tokio::main]
async fn main() -> Result<(), NodeIdentifierError> {
    let _guard = setup_tracing(SERVICE_NAME)?;

    handler().await
}

#[tracing::instrument]
async fn handler() -> Result<(), NodeIdentifierError> {
    let dynamo = DynamoDbClient::from_env();
    let dyn_session_db = SessionDb::new(
        dynamo.clone(),
        std::env::var("GRAPL_DYNAMIC_SESSION_TABLE")?,
    );
    let node_identifier = NodeIdentifier::new(NodeDescriptionIdentifier::new(dyn_session_db, true));

    let consumer_config = ConsumerConfig::parse();
    let producer_config = ProducerConfig::parse();

    tracing::info!(
        message = "Configuring Kafka StreamProcessor",
        consumer_config = ?consumer_config,
        producer_config = ?producer_config,
    );

    // TODO: also construct a stream processor for retries

    let stream_processor: StreamProcessor<GraphDescription, IdentifiedGraph> =
        StreamProcessor::new(consumer_config, producer_config)?;

    tracing::info!(message = "Kafka StreamProcessor configured successfully");

    let stream = stream_processor.stream::<_, _, StreamProcessorError>(
        move |event: Result<(tracing::Span, Envelope<GraphDescription>), StreamProcessorError>| {
            {
                let identifier = node_identifier.clone();
                async move {
                    let (span, envelope) = event?;
                    let handler_span = span.clone();
                    let _guard = span.enter();
                    let tenant_id = envelope.tenant_id();
                    let trace_id = envelope.trace_id();
                    let event_source_id = envelope.event_source_id();
                    let graph_description = envelope.inner_message();

                    tracing::debug!("received kafka message");

                    match identifier
                        .handle_event(tenant_id, graph_description)
                        .instrument(handler_span)
                        .await
                    {
                        Ok(identified_graph) => Ok(Some(Envelope::new(
                            tenant_id,
                            trace_id,
                            event_source_id,
                            identified_graph,
                        ))),
                        Err(e) => match e {
                            Ok((_, e)) => {
                                match e {
                                    NodeIdentifierError::AttributionFailure => {
                                        tracing::warn!(
                                            message = "failed to attribute",
                                            error = %e,
                                        );
                                        // TODO: write message to retry topic here
                                        Err(StreamProcessorError::from(e))
                                    }
                                    _ => {
                                        tracing::error!(
                                            mesage = "unexpected error",
                                            error = %e,
                                        );
                                        // TODO: write message to failed topic here
                                        Err(StreamProcessorError::from(e))
                                    }
                                }
                            }
                            Err(e) => match e {
                                NodeIdentifierError::EmptyGraph => {
                                    tracing::warn!(message = "identified subgraph is empty",);
                                    Ok(None)
                                }
                                _ => {
                                    tracing::error!(
                                        message = "unexpected error",
                                        error = %e,
                                    );
                                    Err(StreamProcessorError::from(e))
                                }
                            },
                        },
                    }
                }
            }
            .into_stream()
            .filter_map(|res| async move { res.transpose() })
        },
    );

    stream
        .for_each(|res| async move {
            if let Err(e) = res {
                // TODO: retry the message?
                tracing::error!(
                    message = "error processing kafka message",
                    reason = %e,
                );
            } else {
                // TODO: collect some metrics
                tracing::debug!(message = "identified graph from graph description");
            }
        })
        .with_current_subscriber()
        .await;

    Ok(())
}

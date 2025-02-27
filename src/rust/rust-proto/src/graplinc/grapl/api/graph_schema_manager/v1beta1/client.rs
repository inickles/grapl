use std::time::Duration;

use client_executor::{
    Executor,
    ExecutorConfig,
};
use tonic::transport::Endpoint;

use crate::{
    client_factory::services::GraphSchemaManagerClientConfig,
    client_macros::RpcConfig,
    create_proto_client,
    execute_client_rpc,
    graplinc::grapl::api::graph_schema_manager::v1beta1::messages as native,
    protobufs::graplinc::grapl::api::graph_schema_manager::{
        v1beta1 as proto,
        v1beta1::graph_schema_manager_service_client::GraphSchemaManagerServiceClient as GraphSchemaManagerServiceClientProto,
    },
    protocol::{
        error::GrpcClientError,
        service_client::{
            ConnectError,
            Connectable,
        },
    },
};

pub type GraphSchemaManagerClientError = GrpcClientError;

#[derive(Clone)]
pub struct GraphSchemaManagerClient {
    proto_client: GraphSchemaManagerServiceClientProto<tonic::transport::Channel>,
    executor: Executor,
}

#[async_trait::async_trait]
impl Connectable for GraphSchemaManagerClient {
    type Config = GraphSchemaManagerClientConfig;
    const SERVICE_NAME: &'static str =
        "graplinc.grapl.api.graph_schema_manager.v1beta1.GraphSchemaManagerService";

    #[tracing::instrument(err)]
    async fn connect_with_endpoint(endpoint: Endpoint) -> Result<Self, ConnectError> {
        let executor = Executor::new(ExecutorConfig::new(Duration::from_secs(30)));
        let proto_client = create_proto_client!(
            executor,
            GraphSchemaManagerServiceClientProto<tonic::transport::Channel>,
            endpoint,
        );

        Ok(Self {
            proto_client,
            executor,
        })
    }
}

impl GraphSchemaManagerClient {
    pub async fn deploy_schema(
        &mut self,
        request: native::DeploySchemaRequest,
    ) -> Result<native::DeploySchemaResponse, GraphSchemaManagerClientError> {
        execute_client_rpc!(
            self,
            request,
            deploy_schema,
            proto::DeploySchemaRequest,
            native::DeploySchemaResponse,
            RpcConfig::default(),
        )
    }

    pub async fn get_edge_schema(
        &mut self,
        request: native::GetEdgeSchemaRequest,
    ) -> Result<native::GetEdgeSchemaResponse, GraphSchemaManagerClientError> {
        execute_client_rpc!(
            self,
            request,
            get_edge_schema,
            proto::GetEdgeSchemaRequest,
            native::GetEdgeSchemaResponse,
            RpcConfig::default(),
        )
    }
}

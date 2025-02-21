use crate::client_factory::grpc_client_config::{
    GenericGrpcClientConfig,
    GrpcClientConfig,
};

#[derive(clap::Parser, Debug)]
pub struct PipelineIngressClientConfig {
    #[clap(long, env)]
    pub pipeline_ingress_client_address: String,
}

impl From<PipelineIngressClientConfig> for GenericGrpcClientConfig {
    fn from(val: PipelineIngressClientConfig) -> Self {
        GenericGrpcClientConfig {
            address: val.pipeline_ingress_client_address,
        }
    }
}

impl GrpcClientConfig for PipelineIngressClientConfig {}

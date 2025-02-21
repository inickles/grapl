use endpoint_plugin::{
    AssetNode,
    FileNode,
    IAssetNode,
    IFileNode,
    IProcessNode,
    ProcessNode,
};
use rust_proto::graplinc::grapl::api::graph::v1beta1::GraphDescription;
use sysmon_parser::{
    event_data::FileCreateEventData,
    System,
};

use crate::{
    error::SysmonGeneratorError,
    models::{
        get_image_name,
        strip_file_zone_identifier,
        utc_to_epoch,
    },
};

/// Creates a subgrqph describing a `FileCreateEvent`
///
/// The subgraph generation for a `FileCreateEvent` includes the following:
/// * A creator `Process` node - denotes the process that created the file
/// * A subject `File` node - the file that is created as part of this event
#[tracing::instrument]
pub(crate) fn generate_file_create_subgraph(
    system: &System,
    event_data: &FileCreateEventData<'_>,
) -> Result<GraphDescription, SysmonGeneratorError> {
    tracing::trace!("generating graph from event");

    let timestamp = utc_to_epoch(&event_data.creation_utc_time)?;
    let mut graph = GraphDescription::new();

    let mut asset = AssetNode::new(AssetNode::static_strategy());
    asset
        .with_asset_id(&system.computer)
        .with_hostname(&system.computer);

    let mut creator = ProcessNode::new(ProcessNode::session_strategy());
    creator
        .with_asset_id(&system.computer)
        .with_process_id(event_data.process_id)
        .with_process_name(get_image_name(&event_data.image))
        .with_last_seen_timestamp(timestamp);

    let mut file = FileNode::new(FileNode::session_strategy());
    file.with_asset_id(&system.computer)
        .with_file_path(strip_file_zone_identifier(&event_data.target_filename))
        .with_created_timestamp(timestamp);

    graph.add_edge(
        "process_asset",
        creator.clone_node_key(),
        asset.clone_node_key(),
    );

    graph.add_edge(
        "created_files",
        creator.clone_node_key(),
        file.clone_node_key(),
    );

    graph.add_edge(
        "files_on_asset",
        asset.clone_node_key(),
        file.clone_node_key(),
    );

    graph.add_node(asset);
    graph.add_node(creator);
    graph.add_node(file);

    Ok(graph)
}

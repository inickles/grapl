use std::collections::HashMap;

use rust_proto::graplinc::grapl::api::graph::v1beta1::{
    NodeProperty,
    Property,
};

pub struct Escaped(String);

impl std::ops::Deref for Escaped {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}

impl std::fmt::Display for Escaped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn escape_quote(s: &str) -> Escaped {
    // otherwise we need to double quote it

    let mut output = String::with_capacity(s.len());
    output.push('"');

    for c in s.chars() {
        if c == '"' {
            output += "\\\"";
        } else if c == '\\' {
            output += "\\\\";
        } else {
            output.push(c);
        }
    }

    output.push('"');
    Escaped(output)
}

fn escape_prop(node_property: &NodeProperty) -> Escaped {
    match &node_property.property {
        Property::IncrementOnlyIntProp(prop) => escape_quote(&prop.to_string()),
        Property::DecrementOnlyIntProp(prop) => escape_quote(&prop.to_string()),
        Property::ImmutableIntProp(prop) => escape_quote(&prop.to_string()),
        Property::IncrementOnlyUintProp(prop) => escape_quote(&prop.to_string()),
        Property::DecrementOnlyUintProp(prop) => escape_quote(&prop.to_string()),
        Property::ImmutableUintProp(prop) => escape_quote(&prop.to_string()),
        Property::ImmutableStrProp(prop) => escape_quote(prop.as_inner()),
    }
}

#[tracing::instrument]
pub(crate) fn build_upserts(
    query_param: u128,
    node_key: &str,
    node_type: &str,
    properties: &HashMap<String, NodeProperty>,
    key_query_map: &mut HashMap<String, String>,
) -> (String, Vec<dgraph_tonic::Mutation>) {
    let _node_key = node_key.to_string();
    let node_key = escape_quote(node_key);
    let mut inner_queries = String::with_capacity(properties.len() * 32);
    let mut mutations = Vec::with_capacity(properties.len());

    let (creation_var_name, creation_query, creation_quad) =
        node_creation_quads(query_param, &node_key, node_type);
    key_query_map.insert(_node_key, creation_var_name.clone());

    mutations.push(creation_quad);
    inner_queries.push_str(&creation_query);
    inner_queries.push('\n');
    for (prop_name, prop) in properties.iter() {
        if prop_name == "node_key" {
            continue;
        }
        if prop_name == "dgraph.type" {
            continue;
        }
        tracing::debug!(
            message="generating upsert quads for predicate",
            predicate_name=?prop_name,
        );
        let prop_value = escape_prop(prop);
        let (next_query, muts) =
            gen_node_property_upsert_quads(&creation_var_name, prop_name, &prop_value);
        inner_queries.push_str(&next_query);
        inner_queries.push('\n');
        mutations.extend_from_slice(&muts[..]);
    }

    (inner_queries, mutations)
}

pub(crate) fn node_creation_quads(
    query_param: u128,
    node_key: &Escaped,
    node_type: &str,
) -> (String, String, dgraph_tonic::Mutation) {
    let creation_var_name = format!("node_exists_{}", query_param);
    let escaped_node_key = node_key;
    let inner_query = format!(
        r#"
            {creation_var_name} as var(func: eq(node_key, {node_key}), first: 1) @cascade
            q_{creation_var_name}(func: uid({creation_var_name}), first: 1) @cascade
            {{
                uid,
                node_key,
            }}
    "#,
        creation_var_name = creation_var_name,
        node_key = escaped_node_key,
    );

    // If the node exists, do nothing, otherwise create it with its type
    let mut mu_1 = dgraph_tonic::Mutation::new();
    let mu_1_n_quads = format!(
        concat!(
            r#"_:{creation_var_name} <node_key> {node_key} ."#,
            "\n",
            r#"_:{creation_var_name} <dgraph.type> "{node_type}" ."#,
        ),
        node_key = escaped_node_key,
        node_type = node_type,
        creation_var_name = creation_var_name,
    );

    mu_1.set_set_nquads(mu_1_n_quads);
    mu_1.set_cond(format!(
        "@if(eq(len({creation_var_name}), 0))",
        creation_var_name = creation_var_name
    ));

    (creation_var_name, inner_query, mu_1)
}

pub(crate) fn gen_node_property_upsert_quads(
    creation_var_name: &str,
    prop_name: &str,
    prop_value: &Escaped,
) -> (String, [dgraph_tonic::Mutation; 2]) {
    // let mut node_query_name = format!("pred_query_{}_{}_{}", prop_name, query_param, predicate_param);
    let mut mu_0 = dgraph_tonic::Mutation::new();

    let inner_query = format!(
        r#"
            var(func: uid({creation_var_name}), first: 1)
    "#,
        creation_var_name = creation_var_name,
    );

    // If the node exists, set the predicate. Currently 'last write wins'.
    let mu_0_n_quads = format!(
        r#"uid({creation_var_name}) <{prop_name}> {prop_value} ."#,
        creation_var_name = creation_var_name,
        prop_name = prop_name,
        prop_value = prop_value,
    );

    mu_0.set_set_nquads(mu_0_n_quads);
    mu_0.set_cond(format!(
        "@if(eq(len({creation_var_name}), 1))",
        creation_var_name = creation_var_name
    ));

    let mut mu_1 = dgraph_tonic::Mutation::new();

    // condition if the node does not exist
    let mu_1_n_quads = format!(
        concat!(r#"_:{creation_var_name} <{prop_name}> {prop_value} ."#,),
        creation_var_name = creation_var_name,
        prop_name = prop_name,
        prop_value = prop_value,
    );

    mu_1.set_set_nquads(mu_1_n_quads);
    mu_1.set_cond(format!(
        "@if(eq(len({creation_var_name}), 0))",
        creation_var_name = creation_var_name
    ));

    (inner_query, [mu_0, mu_1])
}

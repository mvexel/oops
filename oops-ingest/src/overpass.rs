use crate::{IngestError, Result};
use oops_core::{
    BoundingBox, MemberType, OsmData, OsmId, OsmNode, OsmRelation, OsmWay, RelationMember,
};
use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const DEFAULT_USER_AGENT: &str = "OOPS/0.1.0 (https://github.com/mvexel/oops)";

#[derive(Debug)]
pub struct OverpassClient {
    client: Client,
    base_url: String,
    user_agent: String,
}

impl Default for OverpassClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OverpassClient {
    pub fn new() -> Self {
        Self::with_timeout(300)
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        let user_agent =
            std::env::var("OOPS_OVERPASS_USER_AGENT").unwrap_or_else(|_| DEFAULT_USER_AGENT.into());

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent(user_agent.clone())
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: "https://overpass-api.de/api/interpreter".into(),
            user_agent,
        }
    }

    pub async fn fetch_in_bbox(&self, bbox: &BoundingBox, query: &str) -> Result<OsmData> {
        let query = query.trim().trim_end_matches(';');
        let overpass_query = format!(
            "[out:json][bbox:{},{},{},{}];{};out meta;",
            bbox.min_lat, bbox.min_lon, bbox.max_lat, bbox.max_lon, query
        );

        self.execute_query(&overpass_query).await
    }

    pub async fn fetch_by_query(&self, query: &str) -> Result<OsmData> {
        let query = query.trim().trim_end_matches(';');
        let overpass_query = format!("[out:json];{};out meta;", query);
        self.execute_query(&overpass_query).await
    }

    async fn execute_query(&self, query: &str) -> Result<OsmData> {
        let response = self
            .client
            .post(&self.base_url)
            .header(USER_AGENT, &self.user_agent)
            .header(ACCEPT, "application/json")
            .form(&[("data", query)])
            .send()
            .await?;

        if response.status() == 429 {
            return Err(IngestError::RateLimit);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(IngestError::Query(format!(
                "Overpass returned HTTP {}: {}",
                status,
                body.lines().take(20).collect::<Vec<_>>().join(" ")
            )));
        }

        let json: OverpassResponse = response.json().await?;
        self.convert_response(json)
    }

    fn convert_response(&self, response: OverpassResponse) -> Result<OsmData> {
        let mut osm_data = OsmData::new();

        for element in response.elements {
            match element {
                OverpassElement::Node { id, lat, lon, tags } => {
                    let node = OsmNode {
                        id,
                        lat,
                        lon,
                        tags: tags.unwrap_or_default(),
                    };
                    osm_data.add_node(node);
                }
                OverpassElement::Way { id, nodes, tags } => {
                    let way = OsmWay {
                        id,
                        nodes,
                        tags: tags.unwrap_or_default(),
                    };
                    osm_data.ways.insert(id, way);
                }
                OverpassElement::Relation { id, members, tags } => {
                    let our_members = members
                        .into_iter()
                        .map(|member| RelationMember {
                            member_type: match member.element_type.as_str() {
                                "node" => MemberType::Node,
                                "way" => MemberType::Way,
                                "relation" => MemberType::Relation,
                                _ => MemberType::Node,
                            },
                            member_id: member.reference,
                            role: member.role,
                        })
                        .collect();

                    let relation = OsmRelation {
                        id,
                        members: our_members,
                        tags: tags.unwrap_or_default(),
                    };
                    osm_data.relations.insert(id, relation);
                }
            }
        }

        Ok(osm_data)
    }
}

#[derive(Debug, Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum OverpassElement {
    Node {
        id: OsmId,
        lat: f64,
        lon: f64,
        tags: Option<HashMap<String, String>>,
    },
    Way {
        id: OsmId,
        nodes: Vec<OsmId>,
        tags: Option<HashMap<String, String>>,
    },
    Relation {
        id: OsmId,
        members: Vec<OverpassMember>,
        tags: Option<HashMap<String, String>>,
    },
}

#[derive(Debug, Deserialize)]
struct OverpassMember {
    #[serde(rename = "type")]
    element_type: String,
    #[serde(rename = "ref")]
    reference: OsmId,
    role: String,
}

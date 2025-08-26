use crate::{IngestError, Result};
use oops_core::{OsmData, OsmNode, OsmWay, OsmRelation, BoundingBox, OsmId, RelationMember, MemberType};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug)]
pub struct OverpassClient {
    client: Client,
    base_url: String,
}

impl OverpassClient {
    pub fn new() -> Self {
        Self::with_timeout(300)
    }
    
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("OOPS/0.1.0")
            .build()
            .expect("Failed to create HTTP client");
            
        Self {
            client,
            base_url: "https://overpass-api.de/api/interpreter".to_string(),
        }
    }
    
    pub async fn fetch_in_bbox(&self, bbox: &BoundingBox, query: &str) -> Result<OsmData> {
        let overpass_query = format!(
            "[out:json][bbox:{},{},{},{}];{};out meta;",
            bbox.min_lat, bbox.min_lon, bbox.max_lat, bbox.max_lon,
            query
        );
        
        self.execute_query(&overpass_query).await
    }
    
    pub async fn fetch_by_query(&self, query: &str) -> Result<OsmData> {
        let overpass_query = format!("[out:json];{};out meta;", query);
        self.execute_query(&overpass_query).await
    }
    
    async fn execute_query(&self, query: &str) -> Result<OsmData> {
        let response = self.client
            .post(&self.base_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("data={}", query))
            .send()
            .await?;
            
        if response.status() == 429 {
            return Err(IngestError::RateLimit);
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
                    let our_members = members.into_iter().map(|member| {
                        RelationMember {
                            member_type: match member.element_type.as_str() {
                                "node" => MemberType::Node,
                                "way" => MemberType::Way, 
                                "relation" => MemberType::Relation,
                                _ => MemberType::Node,
                            },
                            member_id: member.reference,
                            role: member.role,
                        }
                    }).collect();
                    
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
use geo::{Point, Rect};
use rstar::{RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type OsmId = i64;
pub type Tags = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BoundingBox {
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Self {
        Self {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        }
    }

    pub fn from_rect(rect: Rect<f64>) -> Self {
        Self {
            min_lon: rect.min().x,
            min_lat: rect.min().y,
            max_lon: rect.max().x,
            max_lat: rect.max().y,
        }
    }

    pub fn contains_point(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min_lon && lon <= self.max_lon && lat >= self.min_lat && lat <= self.max_lat
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OsmNode {
    pub id: OsmId,
    pub lat: f64,
    pub lon: f64,
    pub tags: Tags,
}

impl OsmNode {
    pub fn new(id: OsmId, lon: f64, lat: f64) -> Self {
        Self {
            id,
            lat,
            lon,
            tags: HashMap::new(),
        }
    }

    pub fn to_point(&self) -> Point<f64> {
        Point::new(self.lon, self.lat)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OsmWay {
    pub id: OsmId,
    pub nodes: Vec<OsmId>,
    pub tags: Tags,
}

impl OsmWay {
    pub fn new(id: OsmId, nodes: Vec<OsmId>) -> Self {
        Self {
            id,
            nodes,
            tags: HashMap::new(),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.nodes.len() > 2 && self.nodes.first() == self.nodes.last()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OsmRelation {
    pub id: OsmId,
    pub members: Vec<RelationMember>,
    pub tags: Tags,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationMember {
    pub member_type: MemberType,
    pub member_id: OsmId,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemberType {
    Node,
    Way,
    Relation,
}

impl OsmRelation {
    pub fn new(id: OsmId) -> Self {
        Self {
            id,
            members: Vec::new(),
            tags: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct NodeIndex {
    node: OsmNode,
    point: Point<f64>,
}

impl RTreeObject for NodeIndex {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.point.x(), self.point.y()])
    }
}

#[derive(Debug)]
pub struct OsmData {
    pub nodes: HashMap<OsmId, OsmNode>,
    pub ways: HashMap<OsmId, OsmWay>,
    pub relations: HashMap<OsmId, OsmRelation>,
    node_index: RTree<NodeIndex>,
}

impl Default for OsmData {
    fn default() -> Self {
        Self::new()
    }
}

impl OsmData {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            ways: HashMap::new(),
            relations: HashMap::new(),
            node_index: RTree::new(),
        }
    }

    pub fn add_node(&mut self, node: OsmNode) {
        let point = node.to_point();
        let index_entry = NodeIndex {
            node: node.clone(),
            point,
        };

        self.node_index.insert(index_entry);
        self.nodes.insert(node.id, node);
    }

    pub fn get_node(&self, id: OsmId) -> Option<&OsmNode> {
        self.nodes.get(&id)
    }

    pub fn nodes_near_point(&self, lat: f64, lon: f64, radius_meters: f64) -> Vec<&OsmNode> {
        use geo::HaversineDistance;

        let query_point = Point::new(lon, lat);

        // Convert radius to rough degrees for initial spatial filter
        let radius_degrees = radius_meters / 111_000.0; // Rough conversion

        let envelope = AABB::from_corners(
            [lon - radius_degrees, lat - radius_degrees],
            [lon + radius_degrees, lat + radius_degrees],
        );

        self.node_index
            .locate_in_envelope(&envelope)
            .filter(|node_idx| {
                let distance = query_point.haversine_distance(&node_idx.point);
                distance <= radius_meters
            })
            .map(|node_idx| &node_idx.node)
            .collect()
    }

    pub fn nodes_in_bbox(&self, bbox: &BoundingBox) -> Vec<&OsmNode> {
        let envelope =
            AABB::from_corners([bbox.min_lon, bbox.min_lat], [bbox.max_lon, bbox.max_lat]);

        self.node_index
            .locate_in_envelope(&envelope)
            .filter(|node_idx| bbox.contains_point(node_idx.point.x(), node_idx.point.y()))
            .map(|node_idx| &node_idx.node)
            .collect()
    }
}

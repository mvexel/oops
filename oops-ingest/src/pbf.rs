use crate::Result;
use oops_core::{OsmData, OsmNode, OsmWay, OsmRelation, BoundingBox, RelationMember, MemberType};
use osmpbf::{ElementReader, Element};
use std::collections::HashMap;
use std::path::Path;

pub trait ProgressReporter {
    fn report_progress(&self, processed: usize, message: &str);
}

pub struct NoOpProgress;
impl ProgressReporter for NoOpProgress {
    fn report_progress(&self, _processed: usize, _message: &str) {}
}

pub struct ConsoleProgress;
impl ProgressReporter for ConsoleProgress {
    fn report_progress(&self, processed: usize, message: &str) {
        if processed % 10000 == 0 {
            println!("Processed {} elements: {}", processed, message);
        }
    }
}

#[derive(Debug)]
pub struct PbfReader {
    // Could add configuration here
}

impl PbfReader {
    pub fn new() -> Self {
        Self {}
    }
    
    pub async fn read_file<P: AsRef<Path>>(
        &self, 
        path: P, 
        filter: Option<PbfFilter>
    ) -> Result<OsmData> {
        self.read_file_with_progress(path, filter, &NoOpProgress).await
    }
    
    pub async fn read_file_with_progress<P: AsRef<Path>>(
        &self, 
        path: P, 
        filter: Option<PbfFilter>,
        progress: &dyn ProgressReporter
    ) -> Result<OsmData> {
        let filter = filter.unwrap_or_default();
        let reader = ElementReader::from_path(path)?;
        let mut osm_data = OsmData::new();
        let mut count = 0;
        let mut total_processed = 0;
        
        reader.for_each(|element| {
            total_processed += 1;
            // Check limit
            if let Some(limit) = filter.limit {
                if count >= limit {
                    return;
                }
            }
            
            match element {
                Element::Node(node) => {
                    if !filter.include_nodes {
                        return;
                    }
                    
                    let osm_node = OsmNode {
                        id: node.id(),
                        lat: node.lat(),
                        lon: node.lon(),
                        tags: node.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    
                    // Apply filters
                    if !self.node_matches_filter(&osm_node, &filter) {
                        return;
                    }
                    
                    osm_data.add_node(osm_node);
                    count += 1;
                    progress.report_progress(total_processed, &format!("Added {} elements", count));
                }
                Element::DenseNode(node) => {
                    if !filter.include_nodes {
                        return;
                    }
                    
                    let osm_node = OsmNode {
                        id: node.id(),
                        lat: node.lat(),
                        lon: node.lon(),
                        tags: node.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    
                    if !self.node_matches_filter(&osm_node, &filter) {
                        return;
                    }
                    
                    osm_data.add_node(osm_node);
                    count += 1;
                    progress.report_progress(total_processed, &format!("Added {} elements", count));
                }
                Element::Way(way) => {
                    if !filter.include_ways {
                        return;
                    }
                    
                    let osm_way = OsmWay {
                        id: way.id(),
                        nodes: way.refs().collect(),
                        tags: way.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    
                    if !self.way_matches_filter(&osm_way, &filter) {
                        return;
                    }
                    
                    osm_data.ways.insert(osm_way.id, osm_way);
                    count += 1;
                    progress.report_progress(total_processed, &format!("Added {} elements", count));
                }
                Element::Relation(relation) => {
                    if !filter.include_relations {
                        return;
                    }
                    
                    let members = relation.members().map(|member| {
                        RelationMember {
                            member_type: match member.member_type {
                                osmpbf::RelMemberType::Node => MemberType::Node,
                                osmpbf::RelMemberType::Way => MemberType::Way,
                                osmpbf::RelMemberType::Relation => MemberType::Relation,
                            },
                            member_id: member.member_id,
                            role: member.role().unwrap_or("").to_string(),
                        }
                    }).collect();
                    
                    let osm_relation = OsmRelation {
                        id: relation.id(),
                        members,
                        tags: relation.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    
                    if !self.relation_matches_filter(&osm_relation, &filter) {
                        return;
                    }
                    
                    osm_data.relations.insert(osm_relation.id, osm_relation);
                    count += 1;
                    progress.report_progress(total_processed, &format!("Added {} elements", count));
                }
            }
        })?;
        
        Ok(osm_data)
    }
    
    /// Stream elements one by one without loading everything into memory
    pub fn stream_file<P: AsRef<Path>>(
        &self,
        path: P,
        filter: Option<PbfFilter>
    ) -> Result<PbfElementStream> {
        let reader = ElementReader::from_path(path)?;
        Ok(PbfElementStream {
            reader,
            filter: filter.unwrap_or_default(),
        })
    }
    
    fn node_matches_filter(&self, node: &OsmNode, filter: &PbfFilter) -> bool {
        // Check bbox
        if let Some(bbox) = &filter.bbox {
            if !bbox.contains_point(node.lon, node.lat) {
                return false;
            }
        }
        
        // Check required tags
        for (key, value) in &filter.required_tags {
            match value {
                Some(expected) => {
                    if node.tags.get(key) != Some(expected) {
                        return false;
                    }
                }
                None => {
                    if !node.tags.contains_key(key) {
                        return false;
                    }
                }
            }
        }
        
        true
    }
    
    fn way_matches_filter(&self, way: &OsmWay, filter: &PbfFilter) -> bool {
        // Check required tags
        for (key, value) in &filter.required_tags {
            match value {
                Some(expected) => {
                    if way.tags.get(key) != Some(expected) {
                        return false;
                    }
                }
                None => {
                    if !way.tags.contains_key(key) {
                        return false;
                    }
                }
            }
        }
        
        true
    }
    
    fn relation_matches_filter(&self, relation: &OsmRelation, filter: &PbfFilter) -> bool {
        // Check required tags
        for (key, value) in &filter.required_tags {
            match value {
                Some(expected) => {
                    if relation.tags.get(key) != Some(expected) {
                        return false;
                    }
                }
                None => {
                    if !relation.tags.contains_key(key) {
                        return false;
                    }
                }
            }
        }
        
        true
    }
}

#[derive(Debug, Clone)]
pub struct PbfFilter {
    pub include_nodes: bool,
    pub include_ways: bool,
    pub include_relations: bool,
    pub bbox: Option<BoundingBox>,
    pub required_tags: HashMap<String, Option<String>>,
    pub limit: Option<usize>,
}

impl Default for PbfFilter {
    fn default() -> Self {
        Self {
            include_nodes: true,
            include_ways: true,
            include_relations: true,
            bbox: None,
            required_tags: HashMap::new(),
            limit: None,
        }
    }
}

impl PbfFilter {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn nodes_only(mut self) -> Self {
        self.include_nodes = true;
        self.include_ways = false;
        self.include_relations = false;
        self
    }
    
    pub fn ways_only(mut self) -> Self {
        self.include_nodes = false;
        self.include_ways = true;
        self.include_relations = false;
        self
    }
    
    pub fn relations_only(mut self) -> Self {
        self.include_nodes = false;
        self.include_ways = false;
        self.include_relations = true;
        self
    }
    
    pub fn in_bbox(mut self, bbox: &BoundingBox) -> Self {
        self.bbox = Some(bbox.clone());
        self
    }
    
    pub fn with_tag(mut self, key: &str, value: Option<&str>) -> Self {
        self.required_tags.insert(key.to_string(), value.map(|v| v.to_string()));
        self
    }
    
    pub fn limit(mut self, max_elements: usize) -> Self {
        self.limit = Some(max_elements);
        self
    }
}

pub struct PbfElementStream {
    reader: ElementReader<std::io::BufReader<std::fs::File>>,
    filter: PbfFilter,
}

impl PbfElementStream {
    pub fn for_each<F>(self, mut callback: F) -> Result<()>
    where
        F: FnMut(StreamElement),
    {
        self.reader.for_each(|element| {
            match element {
                Element::Node(node) if self.filter.include_nodes => {
                    let osm_node = OsmNode {
                        id: node.id(),
                        lat: node.lat(),
                        lon: node.lon(),
                        tags: node.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    callback(StreamElement::Node(osm_node));
                }
                Element::DenseNode(node) if self.filter.include_nodes => {
                    let osm_node = OsmNode {
                        id: node.id(),
                        lat: node.lat(),
                        lon: node.lon(),
                        tags: node.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    callback(StreamElement::Node(osm_node));
                }
                Element::Way(way) if self.filter.include_ways => {
                    let osm_way = OsmWay {
                        id: way.id(),
                        nodes: way.refs().collect(),
                        tags: way.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    callback(StreamElement::Way(osm_way));
                }
                Element::Relation(relation) if self.filter.include_relations => {
                    let members = relation.members().map(|member| {
                        RelationMember {
                            member_type: match member.member_type {
                                osmpbf::RelMemberType::Node => MemberType::Node,
                                osmpbf::RelMemberType::Way => MemberType::Way,
                                osmpbf::RelMemberType::Relation => MemberType::Relation,
                            },
                            member_id: member.member_id,
                            role: member.role().unwrap_or("").to_string(),
                        }
                    }).collect();
                    
                    let osm_relation = OsmRelation {
                        id: relation.id(),
                        members,
                        tags: relation.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    };
                    callback(StreamElement::Relation(osm_relation));
                }
                _ => {} // Filtered out elements
            }
        })?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum StreamElement {
    Node(OsmNode),
    Way(OsmWay),
    Relation(OsmRelation),
}
use crate::Result;
use oops_core::{BoundingBox, MemberType, OsmData, OsmNode, OsmRelation, OsmWay, RelationMember};
use osmpbf::{Element, ElementReader};
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

#[derive(Default)]
struct PbfReadChunk {
    nodes: Vec<OsmNode>,
    ways: Vec<OsmWay>,
    relations: Vec<OsmRelation>,
    processed: usize,
    added: usize,
}

impl PbfReadChunk {
    fn merge(mut self, mut other: Self) -> Self {
        self.nodes.append(&mut other.nodes);
        self.ways.append(&mut other.ways);
        self.relations.append(&mut other.relations);
        self.processed += other.processed;
        self.added += other.added;
        self
    }
}

impl Default for PbfReader {
    fn default() -> Self {
        Self::new()
    }
}

impl PbfReader {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn read_file<P: AsRef<Path>>(
        &self,
        path: P,
        filter: Option<PbfFilter>,
    ) -> Result<OsmData> {
        self.read_file_with_progress(path, filter, &NoOpProgress)
            .await
    }

    pub async fn read_file_with_progress<P: AsRef<Path>>(
        &self,
        path: P,
        filter: Option<PbfFilter>,
        progress: &dyn ProgressReporter,
    ) -> Result<OsmData> {
        let filter = filter.unwrap_or_default();
        let reader = ElementReader::from_path(&path)?;
        let mut boundary = None;
        
        // Extract boundary from PBF header
        if let Ok(mut blob_reader) = osmpbf::BlobReader::from_path(&path) {
            if let Some(Ok(blob)) = blob_reader.next() {
                if let Ok(header) = blob.to_headerblock() {
                    if let Some(bbox) = header.bbox() {
                        let pbf_boundary = BoundingBox::new(
                            bbox.left,
                            bbox.bottom, 
                            bbox.right,
                            bbox.top
                        );
                        boundary = Some(pbf_boundary);
                        eprintln!("Set PBF boundary: ({:.6}, {:.6}, {:.6}, {:.6})", 
                                 bbox.left, bbox.bottom, bbox.right, bbox.top);
                    }
                }
            }
        }

        let result = reader.par_map_reduce(
            |element| Self::read_element(element, &filter),
            PbfReadChunk::default,
            PbfReadChunk::merge,
        )?;

        progress.report_progress(
            result.processed,
            &format!("Added {} elements", result.added),
        );

        let nodes = result.nodes.into_iter().map(|node| (node.id, node)).collect();
        let ways = result.ways.into_iter().map(|way| (way.id, way)).collect();
        let relations = result
            .relations
            .into_iter()
            .map(|relation| (relation.id, relation))
            .collect();

        Ok(OsmData::from_parts(nodes, ways, relations, boundary))
    }

    /// Stream elements one by one without loading everything into memory
    pub fn stream_file<P: AsRef<Path>>(
        &self,
        path: P,
        filter: Option<PbfFilter>,
    ) -> Result<PbfElementStream> {
        let reader = ElementReader::from_path(path)?;
        Ok(PbfElementStream {
            reader,
            filter: filter.unwrap_or_default(),
        })
    }

    fn read_element(element: Element<'_>, filter: &PbfFilter) -> PbfReadChunk {
        let mut chunk = PbfReadChunk {
            processed: 1,
            ..PbfReadChunk::default()
        };

        match element {
            Element::Node(node) => {
                if !filter.include_nodes {
                    return chunk;
                }

                let osm_node = OsmNode {
                    id: node.id(),
                    lat: node.lat(),
                    lon: node.lon(),
                    tags: node.tags().map(|(k, v)| (k.into(), v.into())).collect(),
                };

                if node_matches_filter(&osm_node, filter) {
                    chunk.nodes.push(osm_node);
                    chunk.added += 1;
                }
            }
            Element::DenseNode(node) => {
                if !filter.include_nodes {
                    return chunk;
                }

                let osm_node = OsmNode {
                    id: node.id(),
                    lat: node.lat(),
                    lon: node.lon(),
                    tags: node.tags().map(|(k, v)| (k.into(), v.into())).collect(),
                };

                if node_matches_filter(&osm_node, filter) {
                    chunk.nodes.push(osm_node);
                    chunk.added += 1;
                }
            }
            Element::Way(way) => {
                if !filter.include_ways {
                    return chunk;
                }

                let osm_way = OsmWay {
                    id: way.id(),
                    nodes: way.refs().collect(),
                    tags: way.tags().map(|(k, v)| (k.into(), v.into())).collect(),
                };

                if way_matches_filter(&osm_way, filter) {
                    chunk.ways.push(osm_way);
                    chunk.added += 1;
                }
            }
            Element::Relation(relation) => {
                if !filter.include_relations {
                    return chunk;
                }

                let members = relation
                    .members()
                    .map(|member| RelationMember {
                        member_type: match member.member_type {
                            osmpbf::RelMemberType::Node => MemberType::Node,
                            osmpbf::RelMemberType::Way => MemberType::Way,
                            osmpbf::RelMemberType::Relation => MemberType::Relation,
                        },
                        member_id: member.member_id,
                        role: member.role().unwrap_or("").into(),
                    })
                    .collect();

                let osm_relation = OsmRelation {
                    id: relation.id(),
                    members,
                    tags: relation.tags().map(|(k, v)| (k.into(), v.into())).collect(),
                };

                if relation_matches_filter(&osm_relation, filter) {
                    chunk.relations.push(osm_relation);
                    chunk.added += 1;
                }
            }
        }

        chunk
    }
}

fn node_matches_filter(node: &OsmNode, filter: &PbfFilter) -> bool {
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

fn way_matches_filter(way: &OsmWay, filter: &PbfFilter) -> bool {
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

fn relation_matches_filter(relation: &OsmRelation, filter: &PbfFilter) -> bool {
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

#[derive(Debug, Clone)]
pub struct PbfFilter {
    pub include_nodes: bool,
    pub include_ways: bool,
    pub include_relations: bool,
    pub bbox: Option<BoundingBox>,
    pub required_tags: HashMap<String, Option<String>>,
}

impl Default for PbfFilter {
    fn default() -> Self {
        Self {
            include_nodes: true,
            include_ways: true,
            include_relations: true,
            bbox: None,
            required_tags: HashMap::new(),
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
        self.required_tags
            .insert(key.into(), value.map(|v| v.into()));
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
                        tags: node
                            .tags()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                    };
                    callback(StreamElement::Node(osm_node));
                }
                Element::DenseNode(node) if self.filter.include_nodes => {
                    let osm_node = OsmNode {
                        id: node.id(),
                        lat: node.lat(),
                        lon: node.lon(),
                        tags: node
                            .tags()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                    };
                    callback(StreamElement::Node(osm_node));
                }
                Element::Way(way) if self.filter.include_ways => {
                    let osm_way = OsmWay {
                        id: way.id(),
                        nodes: way.refs().collect(),
                        tags: way
                            .tags()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                    };
                    callback(StreamElement::Way(osm_way));
                }
                Element::Relation(relation) if self.filter.include_relations => {
                    let members = relation
                        .members()
                        .map(|member| RelationMember {
                            member_type: match member.member_type {
                                osmpbf::RelMemberType::Node => MemberType::Node,
                                osmpbf::RelMemberType::Way => MemberType::Way,
                                osmpbf::RelMemberType::Relation => MemberType::Relation,
                            },
                            member_id: member.member_id,
                            role: member.role().unwrap_or("").into(),
                        })
                        .collect();

                    let osm_relation = OsmRelation {
                        id: relation.id(),
                        members,
                        tags: relation
                            .tags()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
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

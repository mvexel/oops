use crate::Result;
use geo::{BoundingRect, Coord, Intersects, LineString, Rect};
use oops_core::BoundingBox;
use osm_pbf_core::node_store::{NodeStoreReader, NodeStoreWriter};
use osmpbf::{Element, ElementReader};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const ROAD_TYPES: &[&str] = &[
    "motorway",
    "trunk",
    "primary",
    "secondary",
    "tertiary",
    "unclassified",
    "residential",
    "motorway_link",
    "trunk_link",
    "primary_link",
    "secondary_link",
    "tertiary_link",
];

#[derive(Debug, Clone)]
pub struct ResolvedRoadWay {
    pub id: i64,
    pub node_ids: Vec<i64>,
    pub tags: HashMap<String, String>,
    pub geometry: LineString,
}

pub struct LargeEdgeCrossingReader;

impl LargeEdgeCrossingReader {
    pub fn read_road_geometries<P: AsRef<Path>>(
        path: P,
        bbox: Option<&BoundingBox>,
    ) -> Result<Vec<ResolvedRoadWay>> {
        let path = path.as_ref();
        eprintln!("Large mode pass 1/2: indexing nodes to sparse cache...");
        let node_store = Arc::new(index_nodes(path)?);

        eprintln!("Large mode pass 2/2: resolving road geometries...");
        let bbox_rect = bbox.map(bbox_to_rect);
        let reader = ElementReader::from_path(path)?;
        let roads = reader.par_map_reduce(
            |element| read_road_element(element, &node_store, bbox_rect.as_ref()),
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )?;

        eprintln!("Large mode loaded {} road geometries", roads.len());
        Ok(roads)
    }
}

fn index_nodes(path: &Path) -> Result<NodeStoreReader> {
    let reader = ElementReader::from_path(path)?;
    let mut writer =
        NodeStoreWriter::new_sparse().map_err(|err| crate::IngestError::Query(err.to_string()))?;
    let mut count = 0usize;
    let mut error = None;

    reader.for_each(|element| {
        if error.is_some() {
            return;
        }

        let result = match element {
            Element::Node(node) => writer.put(node.id() as u64, node.lat(), node.lon()),
            Element::DenseNode(node) => writer.put(node.id() as u64, node.lat(), node.lon()),
            _ => return,
        };

        match result {
            Ok(()) => count += 1,
            Err(err) => error = Some(err),
        }
    })?;
    if let Some(error) = error {
        return Err(crate::IngestError::Query(error.to_string()));
    }

    eprintln!("Indexed {} nodes", count);
    writer
        .finalize()
        .map_err(|err| crate::IngestError::Query(err.to_string()))
}

fn read_road_element(
    element: Element<'_>,
    node_store: &NodeStoreReader,
    bbox: Option<&Rect>,
) -> Vec<ResolvedRoadWay> {
    let Element::Way(way) = element else {
        return Vec::new();
    };

    let tags: HashMap<String, String> = way.tags().map(|(k, v)| (k.into(), v.into())).collect();
    if !is_road_way(&tags) {
        return Vec::new();
    }

    let node_ids: Vec<i64> = way.refs().collect();
    let coords: Option<Vec<Coord>> = node_ids
        .iter()
        .map(|node_id| {
            node_store
                .get(*node_id as u64)
                .map(|(lon, lat)| Coord { x: lon, y: lat })
        })
        .collect();

    let Some(coords) = coords else {
        return Vec::new();
    };
    if coords.len() < 2 {
        return Vec::new();
    }

    let geometry = LineString::from(coords);
    if let Some(bbox) = bbox {
        let Some(way_rect) = geometry.bounding_rect() else {
            return Vec::new();
        };
        if !bbox.intersects(&way_rect) {
            return Vec::new();
        }
    }

    vec![ResolvedRoadWay {
        id: way.id(),
        node_ids,
        tags,
        geometry,
    }]
}

fn is_road_way(tags: &HashMap<String, String>) -> bool {
    tags.get("highway")
        .map(|highway| ROAD_TYPES.contains(&highway.as_str()))
        .unwrap_or(false)
}

fn bbox_to_rect(bbox: &BoundingBox) -> Rect {
    Rect::new(
        Coord {
            x: bbox.min_lon,
            y: bbox.min_lat,
        },
        Coord {
            x: bbox.max_lon,
            y: bbox.max_lat,
        },
    )
}

"""
Refactored Pedestrian Mapping Quality Analyzer using 6-Level Quality System.

This replaces the monolithic analyzer.py with a clean, modular architecture
that uses configurable quality levels and provides detailed gap analysis.
"""

from __future__ import annotations

import time
from typing import List, Dict, Union, Any, Optional

import geopandas as gpd
import networkx as nx
import pandas as pd
from shapely.geometry import Point
from sklearn.cluster import DBSCAN

from asphalt_core.core.analyzer_interface import AnalysisResult
from asphalt_core.core.base_analyzer import (
    BaseAnalyzer,
    AnalysisIssue,
    BBox,
)
from asphalt_core.core.crs_utils import CRSManager
from asphalt_core.core.data_loader import OSMDataLoader
from asphalt_core.core.spatial_utils import SpatialUtils
from .config import PedestrianConfig
from .quality_assessor import QualityLevelAssessor, QualityResult
from .quality_levels import PedestrianQualityLevel, get_requirements_loader

# Type aliases
NodeId = int
ClusterId = int


class PedestrianAnalyzer(BaseAnalyzer):
    """
    Clean, modular pedestrian mapping quality analyzer using 6-level quality system.
    """

    ANALYSIS_TYPE = "pedestrian"
    DEFAULT_CONFIG_CLASS = PedestrianConfig

    def __init__(
            self,
            config: Optional[PedestrianConfig] = None,
            crs_manager: Optional[CRSManager] = None,
            data_loader: Optional[OSMDataLoader] = None,
            requirements_config_path: Optional[str] = None
    ):
        """Initialize the pedestrian analyzer with quality level assessment."""
        super().__init__()
        self.config = config if config is not None else PedestrianConfig()

        # Core dependencies
        self.crs_manager = crs_manager or CRSManager()
        self.spatial_utils = SpatialUtils(crs_manager=self.crs_manager)
        self.data_loader = data_loader or OSMDataLoader(
            cache_enabled=self.config.cache_osm_data,
            overpass_servers=self.config.overpass_servers,
        )

        # Quality assessment system
        self.requirements_loader = get_requirements_loader(requirements_config_path)
        self.quality_assessor = QualityLevelAssessor(self.requirements_loader)

        # Analysis state
        self._analysis_projected_crs: Optional[str] = None

    @property
    def analysis_type(self) -> str:
        """Analysis type identifier."""
        return "pedestrian"

    @property
    def analyzer_info(self):
        """Return analyzer metadata information."""
        return type("AnalyzerInfo", (), {
            "name": self.analysis_type,
            "version": "2.0.0",
            "display_name": "Pedestrian Quality Levels",
            "description": "6-level quality assessment for pedestrian infrastructure mapping completeness",
            "categories": ["pedestrian", "mapping_quality", "accessibility"],
        })()

    @property
    def default_config(self) -> PedestrianConfig:
        """Default configuration for this analyzer."""
        return PedestrianConfig()

    @property
    def issue_types(self) -> Dict[str, Dict[str, Any]]:
        """Define the 6 quality-based issue types."""
        return {
            "critical": {
                "display_name": "Critical - No Infrastructure",
                "description": "Intersection with no sidewalk infrastructure",
                "severity": "critical",
                "color": "#e74c3c",
                "level": 0
            },
            "minimal": {
                "display_name": "Minimal - Tagged Only",
                "description": "Only sidewalk=* tags on roadways",
                "severity": "high",
                "color": "#f39c12",
                "level": 1
            },
            "basic": {
                "display_name": "Basic - Mapped Sidewalks",
                "description": "Separate sidewalk geometries mapped",
                "severity": "medium",
                "color": "#3498db",
                "level": 2
            },
            "connected": {
                "display_name": "Connected - Has Crossings",
                "description": "Sidewalks connected with tagged crossings",
                "severity": "medium-low",
                "color": "#27ae60",
                "level": 3
            },
            "safety": {
                "display_name": "Safety - Complete Infrastructure",
                "description": "Mapped crossings, curbs, and safety attributes",
                "severity": "low",
                "color": "#2ecc71",
                "level": 4
            },
            "accessibility": {
                "display_name": "Accessibility - Full Details",
                "description": "Complete accessibility information",
                "severity": "info",
                "color": "#16a085",
                "level": 5
            }
        }

    def analyze(
            self, location: Union[str, BBox], config: PedestrianConfig = None
    ) -> AnalysisResult:
        """
        Perform pedestrian mapping quality analysis using 6-level quality system.

        Args:
            location: Place name string or bounding box tuple
            config: Optional configuration override

        Returns:
            AnalysisResult with quality-based findings and detailed gap analysis
        """
        start_time = time.time()
        analysis_config = config or self.config
        location = self.validate_location(location)

        self.logger.info(f"Starting pedestrian quality analysis for {location}")

        try:
            # Extract street network data
            network_data, metadata = self._extract_street_network(location, analysis_config)
            graph, nodes_gdf, edges_gdf = network_data

            # Find intersections
            intersections = self._find_intersections(graph, analysis_config)

            # Analyze intersections using quality levels
            quality_results = self._analyze_intersections(
                nodes_gdf, edges_gdf, intersections, analysis_config
            )

            # Create GeoDataFrame and explanations from quality_results
            features = []
            explanations = {}
            for i, result in enumerate(quality_results):
                feature = {
                    "geometry": Point(result.centroid_lon, result.centroid_lat),
                    "quality_level": result.quality_level.name,
                    "quality_score": result.quality_score,
                    "issue_type": result.issue_type,
                    "representative_osm_id": result.representative_osm_id,
                    "description": f"Quality Level {result.quality_level.value} ({result.quality_level.name})",
                    "cluster_id": result.cluster_id,
                    "missing_infrastructure": result.missing_infrastructure,
                    "actionable_recommendations": result.actionable_recommendations,
                    "next_level_requirements": result.next_level_requirements,
                    "node_count": result.node_count,
                    "crossing_count": result.crossing_count,
                    "curb_count": result.curb_count,
                }
                features.append(feature)
                explanations[i] = result.analysis_details

            if not features:
                gdf = gpd.GeoDataFrame(columns=['geometry'], geometry='geometry', crs="EPSG:4326")
            else:
                gdf = gpd.GeoDataFrame(features, crs="EPSG:4326")

            summary_stats = self._create_summary_statistics(quality_results)
            processing_time = time.time() - start_time

            return AnalysisResult(
                geodata=gdf,
                explanations=explanations,
                location=str(location),
                analysis_type=self.analysis_type,
                total_features=len(gdf),
                summary_stats=summary_stats,
                processing_time_seconds=processing_time,
                osm_data_timestamp=metadata.get("osm_timestamp", ""),
                bounding_box=metadata.get("bbox", (0.0, 0.0, 0.0, 0.0)),
            )

        except Exception as e:
            self.logger.error(f"Analysis failed for {location}: {e}")
            raise

    def _extract_street_network(
            self, location: Union[str, BBox], config: PedestrianConfig
    ) -> tuple:
        """Extract street network with pedestrian infrastructure tags."""
        self.logger.info(f"Extracting street network for {location}")

        # Configure pedestrian tags
        additional_tags = {
            "node": list(config.pedestrian_tags_node),
            "way": list(config.pedestrian_tags_way),
        }

        # Extract network
        network_data, metadata = self.data_loader.extract_network(
            location=location, network_type="all", additional_tags=additional_tags
        )

        graph, nodes_gdf, edges_gdf = network_data
        self.logger.info(f"Extracted {len(nodes_gdf)} nodes and {len(edges_gdf)} edges")

        # Integrate kerb features if available
        try:
            nodes_gdf, _ = self.data_loader.integrate_kerb_features_with_network(
                nodes_gdf, location, kerb_id_offset=config.kerb_id_offset
            )
        except Exception as e:
            self.logger.warning(f"Failed to integrate kerb features: {e}")

        return (graph, nodes_gdf, edges_gdf), metadata

    def _find_intersections(
            self, graph: nx.MultiDiGraph, config: PedestrianConfig
    ) -> List[NodeId]:
        """Find proper road intersections for analysis."""

        # Get nodes with degree > 2
        potential_nodes = [
            node for node, degree in dict(graph.degree()).items() if degree > 2
        ]

        excluded_types = config.excluded_highway_types
        if config.exclude_service_roads:
            excluded_types = excluded_types | config.service_highway_types

        proper_intersections = []

        for node_id in potential_nodes:
            # Get connected highway types
            connected_types = set()
            for _, _, edge_data in graph.edges(node_id, data=True):
                highway_type = edge_data.get("highway")
                if highway_type:
                    if isinstance(highway_type, list):
                        # Safely handle nested lists by flattening and filtering
                        for item in highway_type:
                            if isinstance(item, str):
                                connected_types.add(item)
                            elif isinstance(item, list):
                                # Handle nested lists by adding only string items
                                for subitem in item:
                                    if isinstance(subitem, str):
                                        connected_types.add(subitem)
                    else:
                        if isinstance(highway_type, str):
                            connected_types.add(highway_type)

            # Filter intersections
            non_excluded = connected_types - excluded_types
            major_roads = connected_types & config.major_highway_types

            if config.major_roads_only:
                if len(major_roads) >= 2:
                    proper_intersections.append(node_id)
            else:
                if len(major_roads) > 0 and len(non_excluded) >= 2:
                    proper_intersections.append(node_id)

        self.logger.info(f"Found {len(proper_intersections)} intersections for analysis")
        return proper_intersections

    def _analyze_intersections(
            self,
            nodes_gdf: gpd.GeoDataFrame,
            edges_gdf: gpd.GeoDataFrame,
            intersection_nodes: List[NodeId],
            config: PedestrianConfig,
    ) -> List[QualityResult]:
        """Analyze intersections using quality level assessment."""

        self.logger.info("Analyzing intersections with quality level system")

        # Set projected CRS
        if config.auto_detect_crs:
            bbox = self.crs_manager.get_geodataframe_bounds(nodes_gdf)
            self._analysis_projected_crs = self.crs_manager.get_optimal_projected_crs(
                bbox, config.projected_crs
            )
        else:
            self._analysis_projected_crs = "EPSG:3857"

        # Cluster nearby intersections
        clusters_df = self._cluster_intersections(nodes_gdf, intersection_nodes, config)
        cluster_stats = self._calculate_cluster_stats(clusters_df)

        results = []

        # Assess quality for each cluster
        for cluster_id, stats in cluster_stats.iterrows():
            cluster_nodes = stats["node_id"]
            centroid_x, centroid_y = stats["x"], stats["y"]

            # Get geographic coordinates
            lat, lon, representative_node = self._get_cluster_coordinates(
                cluster_nodes, nodes_gdf, centroid_x, centroid_y
            )

            # Perform quality assessment
            quality_result = self.quality_assessor.assess_intersection(
                cluster_nodes=cluster_nodes,
                nodes_gdf=nodes_gdf,
                edges_gdf=edges_gdf,
                centroid_lat=lat,
                centroid_lon=lon,
                representative_node=representative_node,
                cluster_id=cluster_id
            )

            results.append(quality_result)

        return results

    def _cluster_intersections(
            self,
            nodes_gdf: gpd.GeoDataFrame,
            intersection_nodes: List[NodeId],
            config: PedestrianConfig,
    ) -> pd.DataFrame:
        """Cluster nearby intersections using DBSCAN."""

        if not intersection_nodes:
            return pd.DataFrame(columns=["node_id", "cluster_id", "x", "y"])

        intersection_gdf = nodes_gdf.loc[intersection_nodes].copy()
        projected_gdf = intersection_gdf.to_crs(self._analysis_projected_crs)

        # Extract coordinates for clustering
        coords = [[geom.x, geom.y] for geom in projected_gdf.geometry]

        # Perform DBSCAN clustering
        clustering = DBSCAN(
            eps=config.cluster_distance,
            min_samples=config.min_samples_for_cluster,
        ).fit(coords)

        self.logger.info(f"Created {len(set(clustering.labels_))} clusters")

        return pd.DataFrame({
            "node_id": intersection_gdf.index,
            "cluster_id": clustering.labels_,
            "x": intersection_gdf.geometry.x,
            "y": intersection_gdf.geometry.y,
        })

    def _calculate_cluster_stats(self, clusters_df: pd.DataFrame) -> pd.DataFrame:
        """Calculate cluster statistics."""
        if clusters_df.empty:
            return pd.DataFrame(columns=["x", "y", "node_id"])

        return (
            clusters_df.groupby("cluster_id")
            .agg({"x": "mean", "y": "mean", "node_id": list})
            .round(6)
        )

    def _get_cluster_coordinates(
            self, cluster_nodes: List[NodeId], nodes_gdf: gpd.GeoDataFrame,
            centroid_x: float, centroid_y: float
    ) -> tuple:
        """Convert cluster centroid to geographic coordinates and find representative node."""

        from shapely.geometry import Point

        # Convert centroid to lat/lon
        centroid_point = Point(centroid_x, centroid_y)
        centroid_gdf = gpd.GeoDataFrame([1], geometry=[centroid_point], crs=nodes_gdf.crs)
        centroid_latlon = centroid_gdf.to_crs("EPSG:4326").geometry.iloc[0]
        lat, lon = centroid_latlon.y, centroid_latlon.x

        # Find representative node (closest to centroid)
        cluster_geometries = nodes_gdf.loc[cluster_nodes, "geometry"]
        projected_geoms = cluster_geometries.to_frame('geometry').to_crs(self._analysis_projected_crs)

        projected_centroid = gpd.GeoSeries([centroid_point], crs=nodes_gdf.crs).to_crs(
            self._analysis_projected_crs).iloc[0]
        distances = projected_geoms.geometry.distance(projected_centroid)
        representative_node = distances.idxmin()

        return lat, lon, representative_node

    def _create_summary_statistics(self, quality_results: List[QualityResult]) -> Dict[str, Any]:
        """Create summary statistics focused on quality levels."""

        # Count by quality level
        level_counts = {}
        for level in PedestrianQualityLevel:
            level_counts[level.name.lower()] = 0

        for result in quality_results:
            level_counts[result.quality_level.name.lower()] += 1

        # Calculate quality metrics
        if quality_results:
            avg_quality_score = sum(r.quality_score for r in quality_results) / len(quality_results)
            total_missing_items = sum(
                len(missing.get("ways", {})) + len(missing.get("nodes", {}))
                for r in quality_results
                for missing in [r.missing_infrastructure]
            )
        else:
            avg_quality_score = 0
            total_missing_items = 0

        # Create summary stats that match frontend expectations
        summary_stats = {
            "total_intersections": len(quality_results),
            "average_quality_score": round(avg_quality_score, 1),
            "total_missing_infrastructure_items": total_missing_items,
            "needs_improvement": sum(level_counts[level] for level in ["critical", "minimal", "basic"]),
            "high_quality": sum(level_counts[level] for level in ["safety", "accessibility"])
        }

        # Add individual counts for each quality level (for summary cards)
        for level_name, count in level_counts.items():
            summary_stats[level_name] = count

        return summary_stats
---
title: image

---

# Crossing Quality Levels

- Levels are additive, to qualify for the higher level, all lower level requirements must be met as well as the higher level quality requirements.
- For detecting presence of sidewalks, a buffer distance shall be applied that depends on the `highway` tag
    - 10m for `highway=unclassified` and `highway=pedestrian`
    - 15m for `highway=tertiary`
    - 20m for `highway=secondary`
    - 25m for `highway=primary`
    - 30m for `highway=trunk`

## Definitions
- **Roadway**: `highway` with values `unclassified`, `residential`, `tertiary`, `secondary`, `primary`, `trunk`
- **Simple Intersection**: A node with degree 3 or higher, counting only Roadways
- **Roundabout**: Roadway tagged with `junction=roundabout`
- **Divided Highway** Two Roadways with opposite directionality running parallel within a 25m buffer
- **Complex Intersection**: an intersection involving at least one Divided Highway
- **Intersection**: any of 
    - Simple Intersection
    - Complex Intersection
    - Roundabout
- **Tagged Sidewalk**: a sidewalk defined as `sidewalk=no|left|right|both` tags on a roadway
- **Mapped Sidewalk**: a sidewalk defined as separate OSM way tagged `highway=footway` `footway=sidewalk` running alongside a Roadway
- **Tagged Crossing**: an intersection node connecting a Roadway and a Sidewalk tagged with `highway=crossing`
- **Mapped Crossing**: a way tagged with `highway=crossing` intersecting with one or more Roadways
- **Curb**: a node tagged with `barrier=kerb`

## Level 0: Critical
- Intersection with no Tagged Sidewalks or Mapped Sidewalks present

![image](https://hackmd.io/_uploads/B1w8mW5tlg.png)

### Level 1: Minimal
- Tagged Sidewalks present on all Roadways connected to the Intersection

### Level 2: Basic
- Mapped Sidewalks present along all Roadways connected to the Intersection
- Where Mapped Sidewalks are missing, the absence of sidewalks is confirmed by `sidewalk:left|right=no` on the Roadway

![SCR-20250825-jcyo](https://hackmd.io/_uploads/rkSvP-ctel.jpg)

### Level 3: Connected

- All Mapped Sidewalks are connected across roadways with Tagged Crossings

![SCR-20250825-jdyq](https://hackmd.io/_uploads/BJ00PWcFxx.jpg)

### Level 4: Safety

- All Mapped Sidewalks are connected to Roadways with a Mapped Crossing
- Mapped Sidewalks and Mapped Crossings are connected with a Curb
- Mapped Crossings and Tagged Crossings have the following tags:
    - `traffic_signals`
    - `crossing:island`
    - `crossing:markings`
    - `crossing:island`

### Level 5: Accessibility

- No Tagged Sidewalks exist
- All Mapped Sidewalks have `surface` and `width` taga
- All Mapped Crossings have `surface` and `width` tag
- All Mapped Crossings have accessibility tags:
    - `traffic_signals:sound`
    - `traffic_signals:vibration`
    - `button_operated`
    - `tactile_paving`
    - `wheelchair`
- All Curbs have the following tags:
    - `tactile_paving`
    - `kerb=flush|lowered|raised|no|rolled`


## Reference Documentation

### OpenStreetMap Wiki
- [Sidewalks](https://wiki.openstreetmap.org/wiki/Sidewalks) 
- [Crosswalks](https://wiki.openstreetmap.org/wiki/Crossings)
- [Junctions](https://wiki.openstreetmap.org/wiki/Junctions)
- [`highway=crossing`](https://wiki.openstreetmap.org/wiki/Tag:highway%3Dcrossing)
- [`crossing=*`](https://wiki.openstreetmap.org/wiki/Key:crossing)
- [`crossing:markings`](https://wiki.openstreetmap.org/wiki/Key:crossing:markings)
- [`crossing=traffic_signals`](https://wiki.openstreetmap.org/wiki/Tag:crossing%3Dtraffic_signals)
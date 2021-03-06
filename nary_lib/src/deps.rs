use anyhow::{anyhow, Result};

use petgraph;
use petgraph::graphmap::DiGraphMap;

use bidir_map::BidirMap;

use indexmap::IndexMap;
use serde_json::Value;
use std::{fs::File, io, path::Path};

use crate::{fetch_package_root_metadata, fetch_matching_version_metadata, fetch_package_version_metadata};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dependency {
    pub name: String,
    pub version: String,
}

type DependencyId = i32;

pub fn calculate_depends(
    root_pkg: &Dependency,
    deps: &Vec<Dependency>,
) -> Result<IndexMap<Dependency, ()>> {
    let mut graph: DiGraphMap<DependencyId, i32> = DiGraphMap::new();

    // String doesn't implement Copy and graphmap requires Copy
    let mut map: BidirMap<Dependency, DependencyId> = BidirMap::new();

    map.insert(root_pkg.clone(), 0);

    calculate_depends_rec(root_pkg, deps, &mut map, &mut graph)?;

    let dependency_ids = petgraph::algo::toposort(&graph, None).or_else(|err| {
        Err(anyhow!("Cyclic dependency {:?}", map.get_by_second(&err.node_id())))
    })?;

    let mut ordered_dependencies: IndexMap<Dependency, ()> = IndexMap::new();

    for i in dependency_ids {
        let second = map.get_by_second(&i).unwrap();

        if !ordered_dependencies.contains_key(second) {
            if let Some((dep, _)) = map.remove_by_second(&i) {
                ordered_dependencies.insert(dep.clone(), ());
            }
        }
    }

    Ok(ordered_dependencies)
}

pub fn calculate_depends_rec(
    dependency: &Dependency,
    deps: &Vec<Dependency>,
    map: &mut BidirMap<Dependency, DependencyId>,
    graph: &mut DiGraphMap<DependencyId, i32>,
) -> Result<()> {
    let curr_node = *map.get_by_first(dependency).unwrap();

    if deps.len() == 0 {
        return Ok(());
    }

    let mut remaining_deps = deps.clone();

    while !remaining_deps.is_empty() {
        let index = remaining_deps.len() - 1;
        let dependency = remaining_deps.remove(index);

        println!("{} {}", dependency.name, dependency.version);

        if !map.contains_first_key(&dependency) {
            let dependency_node = map.len() as i32;
            graph.add_node(dependency_node);
            map.insert(dependency, dependency_node);

            graph.add_edge(dependency_node, curr_node, 0);
            let dependency = map.get_mut_by_second(&dependency_node).unwrap().clone();

            let root_metadata = fetch_package_root_metadata(&dependency)?;
            // println!("{}", root_metadata);

            // let versions = &metadata["versions"];
            let matching_version = fetch_matching_version_metadata(&dependency, &root_metadata)?;
            println!("Found version: {}", matching_version.0);

            let package_metadata = fetch_package_version_metadata(&dependency, &matching_version.0)?;
            // pick the version, then install it to get its ["dependencies"]

            // println!("{}", package_metadata);
            let new_deps = serde_json_value_to_dependencies(&package_metadata["dependencies"])?;

            calculate_depends_rec(&dependency, &new_deps, map, graph)?;
        } else {
            let dependency_node = *map.get_by_first(&dependency).unwrap();
            graph.add_edge(dependency_node, curr_node, 0);
        }
    }

    Ok(())
}

pub fn path_to_root_dependency<'a>(file: &Path) -> Result<Dependency> {
    let mut package = file.to_path_buf();

    if !package.ends_with("package.json") {
        package.push("package.json");
    }

    let package_json = File::open(package)?;
    let root: Value = serde_json::from_reader(package_json)?;

    Ok(Dependency {
        name: root["name"].as_str().unwrap().to_string(),
        version: root["version"].as_str().unwrap().to_string()
    })
}

pub fn path_to_dependencies<'a>(file: &Path) -> Result<Vec<Dependency>> {
    let mut package = file.to_path_buf();

    if !package.ends_with("package.json") {
        package.push("package.json");
    }

    let package_json = File::open(package)?;

    json_to_dependencies(&package_json)
}

pub fn json_to_dependencies(mut reader: impl io::Read) -> Result<Vec<Dependency>> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;

    let root: Value = serde_json::from_str(&buffer)?;
    serde_json_value_to_dependencies(&root["dependencies"])
}

pub fn serde_json_value_to_dependencies(root: &serde_json::Value) -> Result<Vec<Dependency>> {
    let mut vec = Vec::new();

    if let Some(dependencies) = root.as_object() {
        for dependency in dependencies.iter() {
            println!("{} {} ", dependency.0, dependency.1);
            if !dependency.0.starts_with("_") {
                vec.push(Dependency {
                    name: dependency.0.to_string(),
                    version: dependency.1.as_str().unwrap().to_string(),
                });
            }
        }
    };

    Ok(vec)
}
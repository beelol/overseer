use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct JobSpec {
    pub id: String,
    pub title: String,
    pub acceptance: String,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub resource_claims: Vec<ResourceClaim>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResourceClaim {
    pub resource: String,
    pub mode: String,
}

/// Keep only independent valid components when the director explicitly opts into a partial
/// initial plan. Every omitted job is reported; no invalid dependency can be dispatched.
pub fn select_valid(raw: &Value) -> Result<(Vec<JobSpec>, Vec<Value>)> {
    let items = raw
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("jobs must be an array"))?;
    if items.len() > 1000 {
        bail!("job backlog exceeds 1000 jobs");
    }
    let mut candidates: Vec<Option<JobSpec>> = Vec::with_capacity(items.len());
    let mut errors: Vec<Option<String>> = Vec::with_capacity(items.len());
    for item in items {
        match serde_json::from_value::<JobSpec>(item.clone()) {
            Ok(job) => {
                let error = basic_error(&job);
                candidates.push(Some(job));
                errors.push(error);
            }
            Err(error) => {
                candidates.push(None);
                errors.push(Some(format!("invalid job: {error}")));
            }
        }
    }
    let mut counts = HashMap::new();
    for (candidate, error) in candidates.iter().zip(&errors) {
        if error.is_none() {
            if let Some(job) = candidate {
                *counts.entry(job.id.as_str()).or_insert(0usize) += 1;
            }
        }
    }
    for (candidate, error) in candidates.iter().zip(&mut errors) {
        if let Some(job) = candidate {
            if counts.get(job.id.as_str()).copied().unwrap_or(0) > 1 {
                *error = Some(format!("duplicate job id {}", job.id));
            }
        }
    }
    loop {
        let active: HashSet<&str> = candidates
            .iter()
            .zip(&errors)
            .filter_map(|(candidate, error)| {
                if error.is_none() {
                    candidate.as_ref().map(|job| job.id.as_str())
                } else {
                    None
                }
            })
            .collect();
        let mut changed = false;
        for (candidate, error) in candidates.iter().zip(&mut errors) {
            if error.is_some() {
                continue;
            }
            if let Some(job) = candidate {
                if let Some(dep) = job.deps.iter().find(|dep| !active.contains(dep.as_str())) {
                    *error = Some(format!("unavailable dependency {dep}"));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut sortable = HashSet::new();
    loop {
        let before = sortable.len();
        for (candidate, error) in candidates.iter().zip(&errors) {
            if error.is_none() {
                if let Some(job) = candidate {
                    if job.deps.iter().all(|dep| sortable.contains(dep)) {
                        sortable.insert(job.id.clone());
                    }
                }
            }
        }
        if sortable.len() == before {
            break;
        }
    }
    for (candidate, error) in candidates.iter().zip(&mut errors) {
        if error.is_none() {
            if let Some(job) = candidate {
                if !sortable.contains(&job.id) {
                    *error = Some("dependency cycle or dependent on cycle".to_string());
                }
            }
        }
    }
    let jobs = candidates
        .iter()
        .zip(&errors)
        .filter_map(|(candidate, error)| {
            if error.is_none() {
                candidate.clone()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if jobs.is_empty() {
        bail!("partial plan has no valid independent jobs");
    }
    let rejected = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            errors[index]
                .as_ref()
                .map(|reason| json!({"index":index,"id":item["id"].as_str(),"reason":reason}))
        })
        .collect();
    Ok((jobs, rejected))
}

fn basic_error(job: &JobSpec) -> Option<String> {
    if job.id.is_empty()
        || job.id.len() > 100
        || !job
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Some("invalid job id".to_string());
    }
    if job.title.trim().is_empty() || job.title.len() > 200 {
        return Some(format!(
            "job {} requires a title of at most 200 characters",
            job.id
        ));
    }
    if job.acceptance.trim().is_empty() || job.acceptance.len() > 4000 {
        return Some(format!("job {} requires an acceptance check", job.id));
    }
    if job.resource_claims.len() > 32 {
        return Some(format!("job {} has too many resource claims", job.id));
    }
    let mut resources = HashSet::new();
    for claim in &job.resource_claims {
        if claim.resource.is_empty()
            || claim.resource.trim() != claim.resource
            || claim.resource.len() > 512
            || claim.resource.chars().any(char::is_control)
            || !resources.insert(claim.resource.as_str())
            || (claim.mode != "read" && claim.mode != "write")
        {
            return Some(format!("job {} has an invalid resource claim", job.id));
        }
    }
    None
}

pub fn validate(jobs: &[JobSpec]) -> Result<()> {
    if jobs.len() > 1000 {
        bail!("job backlog exceeds 1000 jobs");
    }
    let mut ids = HashSet::new();
    for job in jobs {
        if let Some(error) = basic_error(job) {
            bail!(error);
        }
        if !ids.insert(job.id.as_str()) {
            bail!("duplicate job id {}", job.id);
        }
    }
    for job in jobs {
        for dep in &job.deps {
            if !ids.contains(dep.as_str()) {
                bail!("unknown dependency {dep} in job {}", job.id);
            }
        }
    }
    let by_id: HashMap<&str, &JobSpec> = jobs.iter().map(|j| (j.id.as_str(), j)).collect();
    let mut visited = HashSet::new();
    let mut active = HashSet::new();
    fn visit<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a JobSpec>,
        visited: &mut HashSet<&'a str>,
        active: &mut HashSet<&'a str>,
    ) -> Result<()> {
        if visited.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            bail!("dependency cycle at {id}");
        }
        for dep in &by_id[id].deps {
            visit(dep, by_id, visited, active)?;
        }
        active.remove(id);
        visited.insert(id);
        Ok(())
    }
    for job in jobs {
        visit(&job.id, &by_id, &mut visited, &mut active)?;
    }
    Ok(())
}

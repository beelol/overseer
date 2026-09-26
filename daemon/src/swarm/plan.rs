use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Deserialize)]
pub struct JobSpec {
    pub id: String,
    pub title: String,
    pub acceptance: String,
    #[serde(default)]
    pub deps: Vec<String>,
}

pub fn validate(jobs: &[JobSpec]) -> Result<()> {
    if jobs.len() > 1000 {
        bail!("job backlog exceeds 1000 jobs");
    }
    let mut ids = HashSet::new();
    for job in jobs {
        if job.id.is_empty()
            || job.id.len() > 100
            || !job
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            bail!("invalid job id");
        }
        if !ids.insert(job.id.as_str()) {
            bail!("duplicate job id {}", job.id);
        }
        if job.title.trim().is_empty() || job.title.len() > 200 {
            bail!("job {} requires a title of at most 200 characters", job.id);
        }
        if job.acceptance.trim().is_empty() || job.acceptance.len() > 4000 {
            bail!("job {} requires an acceptance check", job.id);
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

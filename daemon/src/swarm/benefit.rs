//! Deterministic fixture preview for choosing a serial or independent parallel batch.
//! Estimates are supplied by the caller; this is not an authoritative admission decision.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Cost {
    elapsed_ms: i64,
    usage_milli: BTreeMap<String, i64>,
}

#[derive(Deserialize)]
struct Worker {
    id: String,
    #[serde(flatten)]
    cost: Cost,
}

#[derive(Deserialize)]
struct Plan {
    planning: Cost,
    context: Cost,
    integration: Cost,
    review: Cost,
    retries: Cost,
    workers: Vec<Worker>,
}

#[derive(Deserialize)]
struct Request {
    independent: bool,
    max_workers: usize,
    serial: Plan,
    parallel: Plan,
    allocation_milli: BTreeMap<String, i64>,
    finishing_reserve_milli: BTreeMap<String, i64>,
}

fn total(
    plan: &Plan,
    parallel: bool,
    units: &[String],
) -> Result<(i64, BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let phases = [
        &plan.planning,
        &plan.context,
        &plan.integration,
        &plan.review,
        &plan.retries,
    ];
    let mut phase_elapsed = 0_i64;
    let mut worker_elapsed = 0_i64;
    let mut usage = units
        .iter()
        .map(|unit| (unit.clone(), 0_i64))
        .collect::<BTreeMap<_, _>>();
    let mut finishing = usage.clone();
    for (index, cost) in phases
        .into_iter()
        .chain(plan.workers.iter().map(|worker| &worker.cost))
        .enumerate()
    {
        if cost.elapsed_ms < 0 || cost.usage_milli.len() != units.len() {
            bail!("negative or incomplete cost estimate");
        }
        if index < 5 {
            phase_elapsed = phase_elapsed
                .checked_add(cost.elapsed_ms)
                .ok_or_else(|| anyhow!("elapsed estimate overflow"))?;
        } else if parallel {
            worker_elapsed = worker_elapsed.max(cost.elapsed_ms);
        } else {
            worker_elapsed = worker_elapsed
                .checked_add(cost.elapsed_ms)
                .ok_or_else(|| anyhow!("elapsed estimate overflow"))?;
        }
        for unit in units {
            let amount = *cost
                .usage_milli
                .get(unit)
                .ok_or_else(|| anyhow!("missing native-unit estimate"))?;
            if amount < 0 || (index >= 5 && amount == 0) {
                bail!("negative or uncalibrated usage estimate");
            }
            let running = usage.get_mut(unit).unwrap();
            *running = running
                .checked_add(amount)
                .ok_or_else(|| anyhow!("usage estimate overflow"))?;
            if index == 2 || index == 3 || index == 4 {
                let reserved = finishing.get_mut(unit).unwrap();
                *reserved = reserved
                    .checked_add(amount)
                    .ok_or_else(|| anyhow!("finishing estimate overflow"))?;
            }
        }
    }
    let elapsed = phase_elapsed
        .checked_add(worker_elapsed)
        .ok_or_else(|| anyhow!("elapsed estimate overflow"))?;
    Ok((elapsed, usage, finishing))
}

pub fn preview(p: &Value) -> Result<Value> {
    let request: Request =
        serde_json::from_value(p.clone()).map_err(|e| anyhow!("invalid benefit request: {e}"))?;
    if request.allocation_milli.is_empty()
        || request.allocation_milli.keys().any(|unit| unit.is_empty())
        || request.allocation_milli.values().any(|amount| *amount < 0)
        || request
            .finishing_reserve_milli
            .keys()
            .ne(request.allocation_milli.keys())
        || request
            .finishing_reserve_milli
            .values()
            .any(|amount| *amount < 0)
    {
        bail!("invalid native-unit allocation or finishing reserve");
    }
    let units = request.allocation_milli.keys().cloned().collect::<Vec<_>>();
    if request.serial.workers.is_empty()
        || request.serial.workers.len() != request.parallel.workers.len()
    {
        bail!("paired plans must estimate the same jobs");
    }
    let mut serial_ids = request
        .serial
        .workers
        .iter()
        .map(|worker| worker.id.as_str())
        .collect::<Vec<_>>();
    let mut parallel_ids = request
        .parallel
        .workers
        .iter()
        .map(|worker| worker.id.as_str())
        .collect::<Vec<_>>();
    serial_ids.sort();
    parallel_ids.sort();
    if serial_ids != parallel_ids
        || serial_ids.iter().any(|id| id.is_empty())
        || serial_ids.windows(2).any(|pair| pair[0] == pair[1])
    {
        bail!("paired plans must estimate distinct matching jobs");
    }
    let (serial_ms, serial_usage, serial_finishing) = total(&request.serial, false, &units)?;
    let (parallel_ms, parallel_usage, finishing_usage) = total(&request.parallel, true, &units)?;
    let serial_affordable = units.iter().all(|unit| {
        serial_usage[unit] <= request.allocation_milli[unit]
            && serial_finishing[unit] <= request.finishing_reserve_milli[unit]
    });
    let parallel_reason = if !request.independent {
        "dependent_jobs"
    } else if request.parallel.workers.len() < 2
        || request.parallel.workers.len() > request.max_workers
    {
        "worker_limit"
    } else if parallel_ms >= serial_ms {
        "no_time_benefit"
    } else if units
        .iter()
        .any(|unit| parallel_usage[unit] > request.allocation_milli[unit])
    {
        "allocation_exceeded"
    } else if units
        .iter()
        .any(|unit| finishing_usage[unit] > request.finishing_reserve_milli[unit])
    {
        "finishing_unaffordable"
    } else {
        "beneficial"
    };
    let reason = if parallel_reason != "beneficial" && !serial_affordable {
        "no_affordable_plan"
    } else {
        parallel_reason
    };
    Ok(json!({
        "decision":if reason == "beneficial" { "parallel" }
            else if serial_affordable { "serial" } else { "blocked" },
        "reason":reason,
        "expected_time_benefit_ms":serial_ms.saturating_sub(parallel_ms),
        "serial":{"elapsed_ms":serial_ms,"usage_milli":serial_usage,
            "finishing_usage_milli":serial_finishing,"affordable":serial_affordable},
        "parallel":{"elapsed_ms":parallel_ms,"usage_milli":parallel_usage,
            "finishing_usage_milli":finishing_usage},
        "authoritative_admission":false,
    }))
}

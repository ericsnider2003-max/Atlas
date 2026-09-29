//! Resolve logical monitor roles to physical monitors at runtime.
//!
//! This is the fix for the hard-coded `id: 1 / id: 2` in the old
//! monitor_layout.yaml: roles claim monitors by geometry, so unplugging a
//! display or changing the Windows arrangement does not break placement.

use crate::config::{FracRect, LayoutsConfig};
use crate::error::{AtlasError, Result};
use crate::platform::{Monitor, PixelRect};
use std::collections::BTreeMap;

pub type RoleMap = BTreeMap<String, Monitor>;

pub fn resolve_roles(cfg: &LayoutsConfig, monitors: &[Monitor]) -> RoleMap {
    let mut unclaimed: Vec<Monitor> = monitors.to_vec();
    let mut map = RoleMap::new();

    for role in &cfg.roles {
        if unclaimed.is_empty() {
            break;
        }
        use crate::config::RoleMatch::*;
        let idx = match role.match_by {
            Primary => unclaimed.iter().position(|m| m.primary),
            Leftmost => min_index(&unclaimed, |m| m.x),
            Rightmost => max_index(&unclaimed, |m| m.x),
            Any => Some(0),
        };
        if let Some(i) = idx {
            map.insert(role.name.clone(), unclaimed.remove(i));
        }
    }

    // Second pass: any role that ran out of monitors collapses onto another
    // role via `fallback_to`, and onto the primary display as a last resort.
    // The map is total afterwards, so placement degrades instead of erroring
    // when you undock.
    let last_resort = monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| monitors.first())
        .copied();

    for role in &cfg.roles {
        if map.contains_key(&role.name) {
            continue;
        }
        let mut hop = role.fallback_to.clone();
        let mut found = None;
        for _ in 0..cfg.roles.len() + 1 {
            let Some(name) = hop else { break };
            if let Some(m) = map.get(&name) {
                found = Some(*m);
                break;
            }
            hop = cfg
                .roles
                .iter()
                .find(|r| r.name == name)
                .and_then(|r| r.fallback_to.clone());
        }
        if let Some(m) = found.or(last_resort) {
            map.insert(role.name.clone(), m);
        }
    }
    map
}

/// Resolve a role name to a monitor, following `fallback_to` chains.
pub fn monitor_for_role(cfg: &LayoutsConfig, roles: &RoleMap, want: &str) -> Result<Monitor> {
    let mut name = want.to_string();
    for _ in 0..cfg.roles.len() + 1 {
        if let Some(m) = roles.get(&name) {
            return Ok(*m);
        }
        let spec = cfg.roles.iter().find(|r| r.name == name);
        match spec.and_then(|s| s.fallback_to.clone()) {
            Some(next) => name = next,
            None => break,
        }
    }
    Err(AtlasError::NoMonitorForRole(want.to_string()))
}

pub fn to_pixels(monitor: &Monitor, frac: FracRect) -> PixelRect {
    PixelRect {
        x: monitor.x + (monitor.width as f32 * frac.x).round() as i32,
        y: monitor.y + (monitor.height as f32 * frac.y).round() as i32,
        width: (monitor.width as f32 * frac.w).round() as i32,
        height: (monitor.height as f32 * frac.h).round() as i32,
    }
}

fn min_index<F: Fn(&Monitor) -> i32>(v: &[Monitor], f: F) -> Option<usize> {
    v.iter()
        .enumerate()
        .min_by_key(|(_, m)| f(m))
        .map(|(i, _)| i)
}

fn max_index<F: Fn(&Monitor) -> i32>(v: &[Monitor], f: F) -> Option<usize> {
    v.iter()
        .enumerate()
        .max_by_key(|(_, m)| f(m))
        .map(|(i, _)| i)
}

/// From the machine's display outputs, each (is it built in, is it active):
/// the built-in screen's state, or `None` when there is no built-in screen.
pub fn built_in_screen_from(outputs: &[(bool, bool)]) -> Option<bool> {
    let built_in: Vec<bool> = outputs.iter().filter(|(internal, _)| *internal).map(|(_, active)| *active).collect();
    if built_in.is_empty() {
        None
    } else {
        Some(built_in.iter().any(|a| *a))
    }
}

use std::collections::{HashMap, HashSet};

pub fn resolve_type_name(names: &HashMap<i32, String>, type_id: i32) -> String {
    names
        .get(&type_id)
        .cloned()
        .unwrap_or_else(|| format!("Type {}", type_id))
}

/// Fitted slot flag ranges in EVE Online.
pub const HIGH_SLOT_START: i32 = 27;
pub const HIGH_SLOT_END: i32 = 34;
pub const MID_SLOT_START: i32 = 19;
pub const MID_SLOT_END: i32 = 26;
pub const LOW_SLOT_START: i32 = 11;
pub const LOW_SLOT_END: i32 = 18;
pub const RIG_SLOT_START: i32 = 92;
pub const RIG_SLOT_END: i32 = 94;
pub const SUBSYSTEM_START: i32 = 125;
pub const SUBSYSTEM_END: i32 = 131;

pub fn is_fitted_slot(flag: i32) -> bool {
    (flag >= LOW_SLOT_START && flag <= LOW_SLOT_END)
        || (flag >= MID_SLOT_START && flag <= MID_SLOT_END)
        || (flag >= HIGH_SLOT_START && flag <= HIGH_SLOT_END)
        || (flag >= RIG_SLOT_START && flag <= RIG_SLOT_END)
        || (flag >= SUBSYSTEM_START && flag <= SUBSYSTEM_END)
}

pub struct FittingCluster {
    pub canonical_idx: usize,
    pub count: usize,
    pub variant_count: usize,
    pub member_indices: Vec<usize>,
}

pub fn cluster_fittings(fittings: &[Vec<(i32, i32)>], threshold: f64) -> Vec<FittingCluster> {
    let fitting_sets: Vec<HashSet<i32>> = fittings
        .iter()
        .map(|f| f.iter().map(|(type_id, _)| *type_id).collect())
        .collect();

    let mut assigned = vec![false; fittings.len()];
    let mut clusters: Vec<FittingCluster> = Vec::new();

    // Count exact duplicates for canonical selection
    let mut exact_counts: HashMap<Vec<i32>, (usize, usize)> = HashMap::new();
    for (i, f) in fittings.iter().enumerate() {
        let mut sorted: Vec<i32> = f.iter().map(|(tid, _)| *tid).collect();
        sorted.sort();
        exact_counts
            .entry(sorted)
            .and_modify(|(c, _)| *c += 1)
            .or_insert((1, i));
    }

    for i in 0..fittings.len() {
        if assigned[i] {
            continue;
        }

        assigned[i] = true;
        let mut members = vec![i];

        for j in (i + 1)..fittings.len() {
            if assigned[j] {
                continue;
            }
            let jaccard = jaccard_similarity(&fitting_sets[i], &fitting_sets[j]);
            if jaccard >= threshold {
                assigned[j] = true;
                members.push(j);
            }
        }

        // Find the canonical fit (most common exact fit in this cluster)
        let mut best_idx = members[0];
        let mut best_count = 0usize;
        for &m in &members {
            let mut sorted: Vec<i32> = fittings[m].iter().map(|(tid, _)| *tid).collect();
            sorted.sort();
            if let Some((count, _)) = exact_counts.get(&sorted) {
                if *count > best_count {
                    best_count = *count;
                    best_idx = m;
                }
            }
        }

        let unique_fits: HashSet<Vec<i32>> = members
            .iter()
            .map(|&m| {
                let mut sorted: Vec<i32> = fittings[m].iter().map(|(tid, _)| *tid).collect();
                sorted.sort();
                sorted
            })
            .collect();

        clusters.push(FittingCluster {
            canonical_idx: best_idx,
            count: members.len(),
            variant_count: unique_fits.len(),
            member_indices: members,
        });
    }

    clusters
}

pub fn jaccard_similarity(a: &HashSet<i32>, b: &HashSet<i32>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

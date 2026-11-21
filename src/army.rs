//! Larva production and army control.

use crate::strategy::BuildOrder;
use crate::ZzzkBot;
use bwapi::{Context, Position, UnitType};

/// Army units that take orders from the push layer.
const ARMY_TYPES: [UnitType; 4] = [
    UnitType::ZERG_ZERGLING,
    UnitType::ZERG_MUTALISK,
    UnitType::ZERG_HYDRALISK,
    UnitType::ZERG_ULTRALISK,
];

/// How far from a wave member an enemy unit becomes the direct target of
/// the wave instead of the march goal (pixels).
const OPPORTUNITY_RADIUS: i32 = 16 * 32;

/// Mutalisk count once the ultra cavern is up: the rest of the gas goes to
/// ultralisks.
const MUTA_CAP: i32 = 12;

/// Ling production ceiling for the ling orders: without it greedy morphing
/// pins minerals below the hatchery and lair prices. Set to the first-wave
/// size: a lower cap starved the double-strength opening push forever.
const LING_CAP: i32 = 32;

/// Hydra ceiling for the hydra rush: same mineral-pinning problem as the
/// ling cap, plus the late game (hive, upgrades) needs the budget.
const HYDRA_CAP: i32 = 24;

/// Morphs an overlord when free supply approaches zero: the margin is half
/// an overlord's capacity, so the morph lands before the block.
pub fn supply_overlords<'game>(
    cx: Context<'_, 'game>,
    bot: &mut ZzzkBot<'game>,
    counts: &crate::util::OwnCounts,
) {
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let free = me.supply_total() - counts.supply_used;
    let margin = UnitType::ZERG_OVERLORD.supply_provided() / 2;
    if free > margin {
        return;
    }
    // One overlord under way is enough: `incomplete` counts egg payloads too.
    if counts.incomplete.get(&UnitType::ZERG_OVERLORD).copied().unwrap_or(0) > 0 {
        return;
    }
    // Retry throttle: a failed morph (usually minerals) would otherwise
    // re-issue and re-log every frame.
    if !bot.build.can_retry(UnitType::ZERG_OVERLORD, game.frame_count()) {
        return;
    }
    if me.minerals() < UnitType::ZERG_OVERLORD.mineral_price() {
        return;
    }

    if let Some(larva) = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if larva.morph(UnitType::ZERG_OVERLORD) {
            bot.log(cx, "supply: overlord morphing");
        } else {
            bot.build.mark_fail(UnitType::ZERG_OVERLORD, game.frame_count());
            bot.log(cx, &format!("overlord morph failed: {}", game.last_error().name()));
        }
    }
}

/// Ling production for the all-in and speedling orders: every idle larva
/// becomes zerglings while minerals last.
pub fn morph_lings<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) -> usize {
    if !matches!(
        bot.strategy.order,
        Some(BuildOrder::Pool4) | Some(BuildOrder::Speedling)
    ) {
        return 0;
    }
    let lings = crate::util::own_counts(&bot.own_units, cx)
        .all
        .get(&UnitType::ZERG_ZERGLING)
        .copied()
        .unwrap_or(0);
    if lings >= LING_CAP {
        return 0;
    }
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let mut made = 0;
    for larva in bot
        .own_units
        .iter(cx)
        .filter(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if me.minerals() < UnitType::ZERG_ZERGLING.mineral_price()
            || lings + made as i32 >= LING_CAP
        {
            break;
        }
        if larva.morph(UnitType::ZERG_ZERGLING) {
            made += 1;
        }
    }
    if made > 0 {
        bot.log(cx, &format!("lings: +{made}"));
    }
    made
}

/// Muta production for the muta rush: idle larvae become mutalisks while
/// minerals and gas last, up to the cap. The cap is unconditional: greedy
/// mutas would otherwise drain the gas below the cavern price forever, so
/// the cavern could never start and the cap (tied to it) never engaged.
pub fn morph_mutas<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    // One count covers both morphing and finished spires: production starts
    // once the spire is on its way.
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let spires = counts.all.get(&UnitType::ZERG_SPIRE).copied().unwrap_or(0);
    if spires == 0 {
        return;
    }
    let mutas = counts.all.get(&UnitType::ZERG_MUTALISK).copied().unwrap_or(0);
    if mutas >= MUTA_CAP {
        return;
    }
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let mut made = 0;
    for larva in bot
        .own_units
        .iter(cx)
        .filter(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if me.minerals() < UnitType::ZERG_MUTALISK.mineral_price()
            || me.gas() < UnitType::ZERG_MUTALISK.gas_price()
        {
            break;
        }
        if larva.morph(UnitType::ZERG_MUTALISK) {
            made += 1;
        } else {
            break;
        }
    }
    if made > 0 {
        bot.log(cx, &format!("mutas: +{made}"));
    }
}

/// Hydra production for the hydra rush: idle larvae become hydralisks while
/// minerals and gas last.
pub fn morph_hydras<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::HydraRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let dens = counts
        .all
        .get(&UnitType::ZERG_HYDRALISK_DEN)
        .copied()
        .unwrap_or(0);
    if dens == 0 {
        return;
    }
    let hydras = counts.all.get(&UnitType::ZERG_HYDRALISK).copied().unwrap_or(0);
    if hydras >= HYDRA_CAP {
        return;
    }
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let mut made = 0;
    for larva in bot
        .own_units
        .iter(cx)
        .filter(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if me.minerals() < UnitType::ZERG_HYDRALISK.mineral_price()
            || me.gas() < UnitType::ZERG_HYDRALISK.gas_price()
            || hydras + made as i32 >= HYDRA_CAP
        {
            break;
        }
        if larva.morph(UnitType::ZERG_HYDRALISK) {
            made += 1;
        } else {
            break;
        }
    }
    if made > 0 {
        bot.log(cx, &format!("hydras: +{made}"));
    }
}

/// Attack-moves the army in waves. With the enemy start known, the goal is
/// that start; unknown — the wave moves to a staging point between the
/// remaining candidate starts, so no time is wasted waiting for the scouts:
/// the army walks toward the enemy while the scouts narrow it down, and
/// anything met on the way is engaged (attack-move).
pub fn push_army<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let game = cx.game();
    // Centroid of the unscouted candidate starts; map center when the set is
    // empty (enemy known to be nowhere among starts — expansions).
    let staging = {
        let locs: Vec<_> = bot.scouting.unscouted.iter().copied().collect();
        if locs.is_empty() {
            Position::new(game.map_width() * 16, game.map_height() * 16)
        } else {
            let n = locs.len() as i32;
            let sum = locs.iter().fold((0, 0), |(sx, sy), loc| {
                (sx + loc.x * 32 + 64, sy + loc.y * 32 + 48)
            });
            Position::new(sum.0 / n, sum.1 / n)
        }
    };
    // The wave goal, by priority: the enemy start when known; otherwise the
    // staging point. Hunt mode (see below) replaces a razed start. Start-box
    // CENTER (+64,+48): the raw tile corner often sits inside the minerals
    // of the adjacent expansion and the pather refuses it.
    let (mut goal, mut target_known) = match bot.scouting.enemy_start {
        Some(loc) => (Position::new(loc.x * 32 + 64, loc.y * 32 + 48), true),
        None => (staging, false),
    };
    let wave: Vec<_> = bot
        .own_units
        .iter(cx)
        .filter(|u| {
            ARMY_TYPES.contains(&u.unit_type())
                && u.exists()
                && u.is_completed()
                && !u.is_morphing()
                // Standing units, plus units taking hits: a wave chewing a
                // building while melee units kill it must re-evaluate, or it
                // dies fixated on the gate.
                && (u.is_idle() || u.is_under_attack())
        })
        .collect();
    // The all-in rushes with the first six lings and flows continuously; the
    // standard openers strike first at double strength, later waves at the
    // normal threshold. Once the all-in is under way every hatchling rides
    // at once — two survivors at the front need help now, not a fresh six
    // gathering at home.
    let all_in = bot.strategy.order == Some(BuildOrder::Pool4);
    let mult = if all_in || bot.pushed { 1 } else { 2 };
    let ling_need = if all_in { 6 } else { 16 };
    let ling_threshold = if all_in && bot.pushed { 1 } else { ling_need * mult };
    let is_wave = |t: UnitType| wave.iter().filter(|u| u.unit_type() == t).count();
    let ready = is_wave(UnitType::ZERG_MUTALISK) >= 4 * mult
        || is_wave(UnitType::ZERG_ULTRALISK) >= 2 * mult
        || is_wave(UnitType::ZERG_HYDRALISK) >= 8 * mult
        || is_wave(UnitType::ZERG_ZERGLING) >= ling_threshold;
    if !ready {
        return;
    }
    let enemy = bot.scouting.enemy;
    // Targets of opportunity: a visible enemy unit near a wave member is
    // attacked directly instead of the march goal — attack-move only
    // engages along the path, not beside it.
    let enemies: Vec<_> = game
        .all_units()
        .iter(cx)
        .filter(|u| {
            u.exists() && u.is_visible() && Some(u.player()) == enemy
                && !u.unit_type().is_building()
                // Ground only: this is the melee wave's target list, and an
                // unreachable flyer (an overlord parked over the base) would
                // both stall the wave and block the hunt.
                && !u.is_flying()
        })
        .collect();
    let enemy_buildings: Vec<_> = game
        .all_units()
        .iter(cx)
        .filter(|u| {
            u.exists() && u.is_visible() && Some(u.player()) == enemy
                && u.unit_type().is_building()
                && !u.is_flying()
        })
        .collect();
    // Hunt mode: the wave stands on its goal with nothing enemy visible
    // anywhere — sweep the remaining bases and mineral clusters and destroy
    // whatever is found. Visited points are logged: without that, the swarm
    // ping-pongs between the two nearest ones forever.
    if !enemies.is_empty() || !enemy_buildings.is_empty() {
        // something to kill: handled per-unit below
    } else if !wave.is_empty() {
        let centroid = wave
            .iter()
            .fold((0, 0), |(sx, sy), u| (sx + u.position().x, sy + u.position().y));
        let centroid = Position::new(centroid.0 / wave.len() as i32, centroid.1 / wave.len() as i32);
        let standing_on_goal = centroid.x.abs_diff(goal.x) < 10 * 32
            && centroid.y.abs_diff(goal.y) < 10 * 32;
        if standing_on_goal {
            bot.hunt_log.push(goal);
            let mut points = hunt_points(cx, bot);
            points.retain(|&p| {
                (p.x.abs_diff(centroid.x) >= 10 * 32 || p.y.abs_diff(centroid.y) >= 10 * 32)
                    && !bot
                        .hunt_log
                        .iter()
                        .any(|&v| (v.x - p.x).abs() + (v.y - p.y).abs() < 8 * 32)
            });
            if points.is_empty() {
                // Full sweep done and the enemy is still hiding: start over.
                bot.hunt_log.clear();
                points = hunt_points(cx, bot);
                points.retain(|&p| {
                    p.x.abs_diff(centroid.x) >= 10 * 32 || p.y.abs_diff(centroid.y) >= 10 * 32
                });
            }
            if let Some(next) = points
                .iter()
                .min_by_key(|&&p| (p.x - centroid.x).abs() + (p.y - centroid.y).abs())
            {
                goal = *next;
                target_known = false; // a hunt move is not the strike
                bot.log(cx, "push: goal empty, hunting next base");
            }
        }
    }
    // Muta micro owns mutalisk orders: wounded ones would only be re-sent
    // to die by the wave, so they are dispatched here instead.
    let mut wave: Vec<_> = wave;
    wave.retain(|u| u.unit_type() != UnitType::ZERG_MUTALISK);
    let size = wave.len();
    // Priority: a visible enemy building first — razed bases are won by
    // killing buildings, units come once they defend.
    let nearest_building = |pos: Position| {
        enemy_buildings
            .iter()
            .copied()
            .min_by_key(|&b| (b.position().x - pos.x).abs() + (b.position().y - pos.y).abs())
    };
    // Unknown enemy start: the wave splits — every unit runs to its nearest
    // unscouted start instead of waiting at a staging point for the scouts.
    // Standing there clears the location (our own vision), an empty one
    // sends the survivors to the next candidate via the hunt below.
    // In hunt mode (no unscouted starts left) the same fan-out spreads the
    // swarm over unchecked bases and expansions — a column would find one
    // base at a time, the fan finds them all at once.
    let mut candidates: Vec<Position> = if target_known {
        Vec::new()
    } else {
        bot.scouting
            .unscouted
            .iter()
            .map(|&loc| Position::new(loc.x * 32 + 64, loc.y * 32 + 48))
            .collect()
    };
    if candidates.is_empty() {
        candidates = hunt_points(cx, bot);
        candidates.retain(|&p| {
            !bot
                .hunt_log
                .iter()
                .any(|&v| (v.x - p.x).abs() + (v.y - p.y).abs() < 8 * 32)
        });
    }
    let frame = game.frame_count();
    let mut fails = 0;
    let mut last_err = String::new();
    // Position telemetry: where the marching mass actually is — the log
    // alone cannot tell "marching slowly" from "wedged at home".
    if frame % 300 == 0 && !wave.is_empty() {
        let (sx, sy) = wave
            .iter()
            .fold((0, 0), |(ax, ay), u| (ax + u.position().x, ay + u.position().y));
        bot.log(
            cx,
            &format!(
                "wave: {} idle at {},{} (goal {},{})",
                wave.len(),
                sx / (32 * wave.len() as i32),
                sy / (32 * wave.len() as i32),
                goal.x / 32,
                goal.y / 32
            ),
        );
    }
    for unit in wave {
        // Melee self-preservation: an enemy unit in weapon range comes
        // FIRST, before any building — chewing a gate while zealots hit
        // back just trades the wave for a building.
        let adjacent = enemies
            .iter()
            .copied()
            .filter(|&e| unit.distance_to_unit(e) < 6 * 32)
            .min_by_key(|&e| unit.distance_to_unit(e));
        if let Some(e) = adjacent {
            let ok = unit.attack_unit(e);
            if !ok {
                fails += 1;
                last_err = game.last_error().name().to_string();
            }
            continue;
        }
        if let Some(bld) = nearest_building(unit.position()) {
            let ok = unit.attack_unit(bld);
            if !ok {
                fails += 1;
                last_err = game.last_error().name().to_string();
            }
            continue;
        }
        let near = enemies
            .iter()
            .copied()
            .filter(|&e| unit.distance_to_unit(e) < OPPORTUNITY_RADIUS)
            .min_by_key(|&e| unit.distance_to_unit(e));
        let ok = match near {
            Some(e) => unit.attack_unit(e),
            None => {
                let unit_goal = candidates
                    .iter()
                    .min_by_key(|&&p| (p.x - unit.position().x).abs() + (p.y - unit.position().y).abs())
                    .copied()
                    .unwrap_or(goal);
                unit.move_to(unit_goal)
            }
        };
        if !ok {
            fails += 1;
            last_err = game.last_error().name().to_string();
        }
    }
    if fails > 0 && frame % 120 == 0 {
        bot.log(cx, &format!("push: {fails} attack orders failed: {last_err}"));
    }
    // A staging move is not the strike: keep the double-strength first wave
    // for the moment the enemy start is actually known.
    if target_known {
        bot.pushed = true;
    }
    let first = if mult == 2 { "first " } else { "" };
    let stage = if target_known { "" } else { ", staging between unknowns" };
    // Small reinforcement waves log throttled: at threshold 1 they fire per
    // hatchling and drown the decision log otherwise.
    if size >= 4 || game.frame_count() % 60 == 0 {
        bot.log(
            cx,
            &format!("push: {first}wave of {size} attack-move to {},{}{stage}", goal.x / 32, goal.y / 32),
        );
    }

    // Mid-course correction: a fresh enemy pin re-aims the marchers once —
    // the wave left for the diagonal before the overlord spotted the enemy,
    // and turns mid-way instead of walking the empty diagonal to the end.
    if target_known && bot.scouting.retarget_fresh {
        bot.scouting.retarget_fresh = false;
        for unit in bot.own_units.iter(cx) {
            if unit.unit_type() != UnitType::ZERG_MUTALISK
                && ARMY_TYPES.contains(&unit.unit_type())
                && unit.exists()
                && unit.is_completed()
                && !unit.is_morphing()
                && !unit.is_attacking()
                && !unit.is_idle()
            {
                unit.attack_position(goal);
            }
        }
        bot.log(cx, "push: fresh pin, marching wave re-aimed");
    }
}

/// Roaming destinations for the hunt: the other start locations plus the
/// mineral clusters (expansion candidates), as cluster centroids.
fn hunt_points<'game>(cx: Context<'_, 'game>, bot: &ZzzkBot<'game>) -> Vec<Position> {
    let game = cx.game();
    let home = Position::new(
        bot.scouting.my_start.x * 32 + 64,
        bot.scouting.my_start.y * 32 + 48,
    );
    let mut points: Vec<Position> = bot
        .scouting
        .start_locations
        .iter()
        .filter(|&&loc| loc != bot.scouting.my_start)
        .map(|&loc| Position::new(loc.x * 32 + 64, loc.y * 32 + 48))
        .collect();
    // Greedy clustering of the static mineral patches: every unassigned
    // patch seeds a cluster that absorbs everything within 12 tiles.
    let mut patches: Vec<Position> = game
        .minerals()
        .iter(cx)
        .filter(|m| m.exists())
        .map(|m| m.position())
        .collect();
    while let Some(&seed) = patches.first() {
        let mut cluster = vec![seed];
        patches.remove(0);
        let mut i = 0;
        while i < patches.len() {
            if cluster.iter().any(|&c| {
                (c.x - patches[i].x).abs() + (c.y - patches[i].y).abs() < 12 * 32
            }) {
                cluster.push(patches.remove(i));
            } else {
                i += 1;
            }
        }
        let n = cluster.len() as i32;
        let (sx, sy) = cluster.iter().fold((0, 0), |(ax, ay), p| (ax + p.x, ay + p.y));
        let center = Position::new(sx / n, sy / n);
        // Skip the home cluster: sending the hunt home wastes the sweep.
        if (center.x - home.x).abs() + (center.y - home.y).abs() > 10 * 32 {
            points.push(center);
        }
    }
    points
}

/// Muta micro: wounded mutas (under 30% hp) retreat home to regen, healthy
/// ones (70%+) focus-fire the lowest-hp enemy in range or march to the enemy
/// start; the 30-70% band keeps the order it already has — no per-unit state
/// needed. Mutas are excluded from the push wave: this function owns their
/// orders.
pub fn muta_micro<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let game = cx.game();
    let mutas: Vec<_> = bot
        .own_units
        .iter(cx)
        .filter(|u| {
            u.exists() && u.is_completed() && !u.is_morphing()
                && u.unit_type() == UnitType::ZERG_MUTALISK
        })
        .collect();
    if mutas.is_empty() {
        return;
    }
    let home = Position::new(
        bot.scouting.my_start.x * 32,
        bot.scouting.my_start.y * 32,
    );
    // Focus target: the weakest visible enemy unit near any muta. All
    // healthy mutas shoot the same one, so nothing is wasted switching.
    let mut target = None;
    if let Some(enemy) = bot.scouting.enemy {
        target = game
            .all_units()
            .iter(cx)
            .filter(|u| {
                u.exists() && u.is_visible() && u.player() == enemy
                    && !u.unit_type().is_building()
            })
            .filter(|&e| mutas.iter().any(|m| m.distance_to_unit(e) < 20 * 32))
            .min_by_key(|&e| e.hit_points());
    }
    let goal = bot.scouting.enemy_start;
    for m in mutas {
        let max = m.unit_type().max_hit_points();
        let hp = m.hit_points();
        if hp * 10 < max * 3 {
            // Retreat: home regen heals; idle there means already arrived.
            if m.distance_to_position(home) > 4 * 32 {
                m.move_to(home);
            }
        } else if hp * 10 > max * 7 && m.is_idle() {
            if let Some(t) = target {
                m.attack_unit(t);
            } else if let Some(goal) = goal {
                m.attack_position(Position::new(goal.x * 32 + 64, goal.y * 32 + 48));
            }
        }
    }
}

/// Ultra production once the cavern is up: idle larvae become ultralisks
/// while minerals and gas last.
pub fn morph_ultras<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    let caverns = crate::util::own_counts(&bot.own_units, cx)
        .all
        .get(&UnitType::ZERG_ULTRALISK_CAVERN)
        .copied()
        .unwrap_or(0);
    if caverns == 0 {
        return;
    }
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let mut made = 0;
    for larva in bot
        .own_units
        .iter(cx)
        .filter(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if me.minerals() < UnitType::ZERG_ULTRALISK.mineral_price()
            || me.gas() < UnitType::ZERG_ULTRALISK.gas_price()
        {
            break;
        }
        if larva.morph(UnitType::ZERG_ULTRALISK) {
            made += 1;
        } else {
            break;
        }
    }
    if made > 0 {
        bot.log(cx, &format!("ultras: +{made}"));
    }
}

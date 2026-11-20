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

/// Attack-moves the army to the enemy start in waves: the push fires only
/// when enough units stand idle (mutas 4, ultras 2, hydras 8, lings 16), and
/// sends all of them at once. Freshly built units hold at the base until the
/// next wave gathers — a trickle of singles dies one by one on the way.
pub fn push_army<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let Some(target) = bot.scouting.enemy_start else {
        return;
    };
    let wave: Vec<_> = bot
        .own_units
        .iter(cx)
        .filter(|u| {
            ARMY_TYPES.contains(&u.unit_type())
                && u.exists()
                && u.is_completed()
                && !u.is_morphing()
                // Only standing units: in-combat and marching ones keep
                // their orders, and re-issuing each frame spams the log.
                && u.is_idle()
        })
        .collect();
    // The first wave strikes at double strength; later waves fire at the
    // normal threshold so reinforcements keep flowing.
    let mult = if bot.pushed { 1 } else { 2 };
    let is_wave = |t: UnitType| wave.iter().filter(|u| u.unit_type() == t).count();
    let ready = is_wave(UnitType::ZERG_MUTALISK) >= 4 * mult
        || is_wave(UnitType::ZERG_ULTRALISK) >= 2 * mult
        || is_wave(UnitType::ZERG_HYDRALISK) >= 8 * mult
        || is_wave(UnitType::ZERG_ZERGLING) >= 16 * mult;
    if !ready {
        return;
    }
    let goal = Position::new(target.x * 32, target.y * 32);
    let size = wave.len();
    for unit in wave {
        unit.attack_position(goal);
    }
    let first = if mult == 2 { "first " } else { "" };
    bot.pushed = true;
    bot.log(
        cx,
        &format!("push: {first}wave of {size} attack-move to {},{}", target.x, target.y),
    );
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

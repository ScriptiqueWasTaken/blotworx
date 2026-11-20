//! EM1 retail ending input tables and movie order.
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub(super) const PIN_GLOBALS: [&str; 57] = [
    "DBC_1st_ZoneA_RedChestBronzePin",
    "DBC_1st_ZoneC_CalvinGoldPin",
    "DBC_ZoneD_BronzePin",
    "DBC_ZoneF_BronzePin",
    "DBC_ZoneL_BronzePin",
    "DBC_ZoneM_GoldPin",
    "DBC_ZoneO_SilverPin",
    "DBC_ZoneR_BronzePin1",
    "DBC_ZoneR_BronzePin2",
    "DBC_ZoneS_GoldPin",
    "MS_PinBronzeRooftop",
    "MS_PinSilverTrainStation",
    "MS_PinGoldMuseum",
    "MS_PinBronzeUsher",
    "MS_PinSilverUsher",
    "MS_PinGoldUsher",
    "TT_V1_BronzePin",
    "MJM_Heaps_SilverPin1",
    "MJM_Piles_SilverPin1",
    "MJM_Piles_GoldPin1",
    "MJM_Slopes_GoldPin1",
    "MJM_Caverns_SilverPin1",
    "MJM_Caverns_GoldPin1",
    "AL_V2_DamienSaltMoving_SilverPin",
    "AL_V2_DamienSaltTiki_SilverPin",
    "AL_V2_BosunBlakeMoving_SilverPin",
    "AL_V2_JimthePuzzledRiddle_GoldPin",
    "AL_DamienChest_GoldPin",
    "potw_Tortooga_Pin_Bronze01",
    "potw_Tortooga_Pin_Bronze02",
    "potw_jungle_pinGold01",
    "potw_PirateVoyage_Pin_Bronze01",
    "potw_PirateVoyage_Pin_Silver01",
    "potw_SR_SilverPin_01",
    "potw_SR_GoldPin_01",
    "Hook_Bronze_Pin",
    "BE_V1_PinGold_01",
    "BE_V2_PinBronze_01",
    "HM_ManorHouseChestOpened",
    "HM_Foyer_SilverPinChest",
    "HM_Stretch_GoldPinChest",
    "HM_Library_SilverPinLeona",
    "HM_BallroomRedChestOpen",
    "HM_Attic_GoldPin_Chest",
    "GV_ZoneB_PinCollected",
    "GV_ZoneC_SecondChest",
    "GV_ZoneC_FirstChest",
    "GV_ZoneF_Chest_Bronze",
    "GV_ZoneF_Chest_Gold",
    "GV_ZoneF_GusChest",
    "GV_ZoneI_ArenaChest",
    "GV_ZoneI_FlowerChest",
    "TL_D1_Bronzepin",
    "TL_D2_Bronzepin",
    "TL_SV_SilverPin",
    "TL_SV_BronzePin",
    "TL_Square_red_Chest2",
];

/// RELba148 visits the slot's ending bytes in presentation order, which
/// differs from the order in which StoreEndgameData captures them.
pub(super) fn movies(outcomes: [u8; 9], settings: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let mut movies = vec!["Movies/EndgameVar_Opening.bik".to_owned()];
    for (index, name) in [
        (6, "SmallPete"), (5, "Clocktower"), (4, "Hook"), (3, "Petetronic"),
        (8, "Pete"), (7, "Horace"), (0, "Daisy"), (2, "Goofy"), (1, "Donald"),
    ] {
        let outcome = if outcomes[index] == 0 { "EndNeg" } else { "EndPos" };
        let key = format!("Extras_{name}_Endgame_{outcome}");
        let path = settings.get(&key).filter(|path| !path.is_empty())
            .with_context(|| format!("Missing original ending movie setting {key}"))?;
        movies.push(path.clone());
    }
    movies.push("Movies/EndgameVar_Closing.bik".into());
    Ok(movies)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ending_interleaves_the_original_outcome_slots() -> Result<()> {
        let names = ["SmallPete", "Clocktower", "Hook", "Petetronic", "Pete", "Horace", "Daisy", "Goofy", "Donald"];
        let settings = names.into_iter().flat_map(|name| ["EndPos", "EndNeg"].map(|ending| {
            (format!("Extras_{name}_Endgame_{ending}"), format!("{name}/{ending}"))
        })).collect();
        assert_eq!(
            movies([1, 0, 1, 1, 0, 0, 1, 1, 0], &settings)?,
            [
                "Movies/EndgameVar_Opening.bik", "SmallPete/EndPos", "Clocktower/EndNeg",
                "Hook/EndNeg", "Petetronic/EndPos", "Pete/EndNeg", "Horace/EndPos",
                "Daisy/EndPos", "Goofy/EndPos", "Donald/EndNeg", "Movies/EndgameVar_Closing.bik",
            ]
        );
        Ok(())
    }
}

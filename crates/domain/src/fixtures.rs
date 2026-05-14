use crate::{
    ClockState, ClockTemplate, Fact, FactSource, FactVisibility, Faction, FactionIdentity,
    FactionState, InventoryItem, Location, Npc, NpcState, NpcStatus, Quest, QuestState,
    QuestStatus, RelationshipState, RoleIdentity, Scenario, ScenarioType, Secret, WorldState,
    WorldStateDelta,
};
use uuid::Uuid;

pub fn scenario() -> ScenarioBuilder {
    ScenarioBuilder {
        scenario: Scenario {
            id: Uuid::new_v4(),
            title: "Guild Registration".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "A high fantasy guild hall under quiet tension.".into(),
            tone: "focused, consequence-driven".into(),
            rules: vec![],
            locations: vec![Location {
                id: "guildhall".into(),
                name: "Guildhall".into(),
                description: "A crowded registration hall full of witnesses.".into(),
                visible: true,
            }],
            factions: vec![Faction {
                id: "guild".into(),
                name: "Continental Adventurer Guild".into(),
                description: "Controls registration and public order.".into(),
                faction_identity: FactionIdentity {
                    public_goal: "process registrations and protect civilians".into(),
                    hidden_goal: Some("monitor dangerous anomalies".into()),
                    values: vec!["order".into(), "contracts".into()],
                    fears: vec!["public panic".into()],
                    methods: vec!["formal exams".into()],
                },
                initial_standing: 0,
            }],
            npcs: vec![Npc {
                id: "examiner".into(),
                name: "Guild Examiner".into(),
                description: "A veteran examiner with a careful eye.".into(),
                role_identity: RoleIdentity {
                    core_emotion: "alert".into(),
                    motivation: "protect civilians while evaluating the player".into(),
                    worldview: "power demands accountability".into(),
                    fear: Some("an uncontrolled magical catastrophe".into()),
                    desire: None,
                    speech_style: "measured and formal".into(),
                    boundaries: vec!["will not ignore public danger".into()],
                    values: vec!["order".into()],
                },
                stats: None,
                initial_status: NpcStatus::Active,
                initial_location_id: Some("guildhall".into()),
                initial_visible_to_player: true,
            }],
            quests: vec![Quest {
                id: "register".into(),
                title: "Register at the Guild".into(),
                description: "Complete the registration process.".into(),
                objectives: vec![],
                visible: true,
            }],
            secrets: vec![],
            clocks: vec![ClockTemplate {
                id: "fame".into(),
                title: "The player's fame spreads".into(),
                current: 0,
                max: 4,
                consequence: "Major factions start treating the player as a strategic threat."
                    .into(),
            }],
        },
    }
}

pub fn world_state(scenario: &Scenario) -> WorldStateBuilder {
    let current_location_id = scenario
        .locations
        .first()
        .map(|location| location.id.clone());
    let active_speaker_id = scenario.npcs.first().map(|npc| npc.id.clone());
    let facts = scenario
        .secrets
        .iter()
        .map(|secret| Fact {
            id: secret.id.clone(),
            text: secret.text.clone(),
            visibility: FactVisibility::GmOnly,
            known_by: vec![],
            source: FactSource::Scenario,
            reveal_conditions: secret.reveal_conditions.clone(),
            related_secret_ids: vec![],
            reveal_condition_satisfied: None,
        })
        .collect();
    let npcs = scenario
        .npcs
        .iter()
        .map(|npc| NpcState {
            npc_id: npc.id.clone(),
            status: npc.initial_status,
            visible_to_player: npc.initial_visible_to_player,
            location_id: npc.initial_location_id.clone(),
            attitude_to_player: None,
            known_facts: vec![],
            notes: vec![],
        })
        .collect();
    let factions = scenario
        .factions
        .iter()
        .map(|faction| FactionState {
            faction_id: faction.id.clone(),
            standing: faction.initial_standing,
            public_notes: vec![],
            hidden_notes: vec![],
            revealed_goals: vec![],
        })
        .collect();
    let quests = scenario
        .quests
        .iter()
        .map(|quest| QuestState {
            quest_id: quest.id.clone(),
            status: QuestStatus::Available,
            completed_objectives: vec![],
            visible: quest.visible,
        })
        .collect();
    let clocks = scenario
        .clocks
        .iter()
        .map(|clock| ClockState {
            id: clock.id.clone(),
            title: clock.title.clone(),
            current: clock.current,
            max: clock.max,
            consequence: clock.consequence.clone(),
            visible_to_player: true,
        })
        .collect();

    WorldStateBuilder {
        state: WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 1,
            current_location_id,
            current_scene: None,
            active_speaker_id,
            facts,
            npcs,
            factions,
            quests,
            clocks,
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        },
    }
}

pub fn empty_delta() -> WorldStateDelta {
    WorldStateDelta::default()
}

pub struct ScenarioBuilder {
    scenario: Scenario,
}

impl ScenarioBuilder {
    pub fn with_id(mut self, id: impl Into<Uuid>) -> Self {
        self.scenario.id = id.into();
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.scenario.title = title.into();
        self
    }

    pub fn with_setting(mut self, setting: impl Into<String>) -> Self {
        self.scenario.setting = setting.into();
        self
    }

    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.scenario.rules.push(rule.into());
        self
    }

    pub fn with_location(self, id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let location = Location {
            id: id.clone(),
            name: name.into(),
            description: format!("{id} description"),
            visible: true,
        };
        self.with_replaced_location(location)
    }

    pub fn with_npc(self, id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let npc = Npc {
            id: id.clone(),
            name: name.into(),
            description: format!("{id} description"),
            role_identity: RoleIdentity {
                core_emotion: "guarded".into(),
                motivation: "maintain control".into(),
                worldview: "actions reveal character".into(),
                fear: None,
                desire: None,
                speech_style: "precise".into(),
                boundaries: vec![],
                values: vec!["discipline".into()],
            },
            stats: None,
            initial_status: NpcStatus::Active,
            initial_location_id: self
                .scenario
                .locations
                .first()
                .map(|location| location.id.clone()),
            initial_visible_to_player: true,
        };
        self.with_replaced_npc(npc)
    }

    pub fn with_faction(self, id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let faction = Faction {
            id: id.clone(),
            name: name.into(),
            description: format!("{id} description"),
            faction_identity: FactionIdentity {
                public_goal: "hold the line".into(),
                hidden_goal: None,
                values: vec!["discipline".into()],
                fears: vec![],
                methods: vec![],
            },
            initial_standing: 0,
        };
        self.with_replaced_faction(faction)
    }

    pub fn with_quest(self, id: impl Into<String>, title: impl Into<String>) -> Self {
        let id = id.into();
        let quest = Quest {
            id: id.clone(),
            title: title.into(),
            description: format!("{id} description"),
            objectives: vec![],
            visible: true,
        };
        self.with_replaced_quest(quest)
    }

    pub fn with_clock(self, id: impl Into<String>, segments: u8) -> Self {
        let id = id.into();
        let clock = ClockTemplate {
            id: id.clone(),
            title: id.clone(),
            current: 0,
            max: segments,
            consequence: format!("{id} consequence"),
        };
        self.with_replaced_clock(clock)
    }

    pub fn with_secret(self, id: impl Into<String>, text: impl Into<String>) -> Self {
        let secret = Secret {
            id: id.into(),
            text: text.into(),
            reveal_conditions: vec![],
        };
        self.with_replaced_secret(secret)
    }

    pub fn build(self) -> Scenario {
        self.scenario
    }

    fn with_replaced_location(mut self, location: Location) -> Self {
        let id = location.id.clone();
        replace_by_id(&mut self.scenario.locations, &id, |item| &item.id, location);
        self
    }

    fn with_replaced_npc(mut self, npc: Npc) -> Self {
        let id = npc.id.clone();
        replace_by_id(&mut self.scenario.npcs, &id, |item| &item.id, npc);
        self
    }

    fn with_replaced_faction(mut self, faction: Faction) -> Self {
        let id = faction.id.clone();
        replace_by_id(&mut self.scenario.factions, &id, |item| &item.id, faction);
        self
    }

    fn with_replaced_quest(mut self, quest: Quest) -> Self {
        let id = quest.id.clone();
        replace_by_id(&mut self.scenario.quests, &id, |item| &item.id, quest);
        self
    }

    fn with_replaced_clock(mut self, clock: ClockTemplate) -> Self {
        let id = clock.id.clone();
        replace_by_id(&mut self.scenario.clocks, &id, |item| &item.id, clock);
        self
    }

    fn with_replaced_secret(mut self, secret: Secret) -> Self {
        let id = secret.id.clone();
        replace_by_id(&mut self.scenario.secrets, &id, |item| &item.id, secret);
        self
    }
}

pub struct WorldStateBuilder {
    state: WorldState,
}

impl WorldStateBuilder {
    pub fn with_session_id(mut self, id: Uuid) -> Self {
        self.state.session_id = id;
        self
    }

    pub fn with_version(mut self, version: u64) -> Self {
        self.state.version = version as i64;
        self
    }

    pub fn with_fact(mut self, fact: Fact) -> Self {
        let id = fact.id.clone();
        replace_by_id(&mut self.state.facts, &id, |item| &item.id, fact);
        self
    }

    pub fn with_recent_event(mut self, text: impl Into<String>) -> Self {
        self.state.recent_events.push(text.into());
        self
    }

    pub fn with_npc_state(mut self, state: NpcState) -> Self {
        let id = state.npc_id.clone();
        replace_by_id(&mut self.state.npcs, &id, |item| &item.npc_id, state);
        self
    }

    pub fn with_faction_state(mut self, state: FactionState) -> Self {
        let id = state.faction_id.clone();
        replace_by_id(
            &mut self.state.factions,
            &id,
            |item| &item.faction_id,
            state,
        );
        self
    }

    pub fn with_quest_state(mut self, state: QuestState) -> Self {
        let id = state.quest_id.clone();
        replace_by_id(&mut self.state.quests, &id, |item| &item.quest_id, state);
        self
    }

    pub fn with_clock_state(mut self, state: ClockState) -> Self {
        let id = state.id.clone();
        replace_by_id(&mut self.state.clocks, &id, |item| &item.id, state);
        self
    }

    pub fn build(self) -> WorldState {
        self.state
    }
}

fn replace_by_id<T, F>(items: &mut Vec<T>, id: &str, get_id: F, value: T)
where
    F: Fn(&T) -> &str,
{
    if let Some(existing) = items.iter_mut().find(|item| get_id(item) == id) {
        *existing = value;
    } else {
        items.push(value);
    }
}

#[allow(dead_code)]
fn _keep_types_visible(_: (&[RelationshipState], &[InventoryItem])) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_scenario;

    #[test]
    fn fixture_builders_create_valid_scenario_and_state() {
        let scenario = scenario().with_secret("void-mark", "Hidden truth").build();
        validate_scenario(&scenario).expect("fixture scenario validates");

        let state = world_state(&scenario).build();
        assert_eq!(state.scenario_id, scenario.id);
        assert!(state.facts.iter().any(|fact| fact.id == "void-mark"));
    }
}

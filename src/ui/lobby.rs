//! The lobby: picking the match, the teams, and who is in.

use super::*;

// The players fill the lobby: in the team modes the screen splits into the
// blue team's half on the left and the red team's on the right, a line for every slot, and in free for all
// everyone is in two columns. What the match will be is on a button in the
// top left corner, which opens the picker for it.

/// The pieces of text in the lobby that change.
#[derive(Component, Clone, Copy)]
pub(super) enum LobbyText {
    /// Whose lobby it is, and who picks the match.
    Caption,
    /// On the match button: what a click on it does.
    MatchHint,
    Mode,
    /// On the match button, under the mode: the map and air acceleration.
    MatchLine,
    /// In the match picker: who can change it.
    PickerNote,
    Bots,
    /// How the last match ended.
    Result,
    /// Over the last match's scoreboard: its map and mode.
    FinalCaption,
    /// Who we are waiting for.
    Waiting,
}

/// Shown only to the lobby leader.
#[derive(Component)]
pub(super) struct LeaderOnly;

/// Shown to everyone but the lobby leader.
#[derive(Component)]
pub(super) struct GuestOnly;

/// Shown only in the team modes.
#[derive(Component)]
pub(super) struct TeamModeOnly;

/// Shown only where there is fighting (and so bots): not in the racing modes.
#[derive(Component)]
pub struct ArmedOnly;

/// Shown only in the racing modes.
#[derive(Component)]
pub(super) struct CourseOnly;

/// The box with how the last match ended; hidden before the first.
#[derive(Component)]
pub(super) struct ResultBanner;

/// The text of the button that copies the lobby's address.
#[derive(Component)]
pub(super) struct CopyLabel;

/// A chip or card with which the leader picks the match.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(crate) enum MatchChoice {
    Mode(u8),
    Map(u8),
    AirAccel(u16),
    /// Whether the bots hold their fire.
    BotsHoldFire(bool),
    /// How well the bots fight (`BOT_DIFFICULTY_NAMES`).
    BotDifficulty(u8),
}

/// Under a map's name: whether it is the one picked.
#[derive(Component)]
pub(super) struct MapNote(pub(super) u8);

/// Whether the match picker (the mode, map, air acceleration and bots) is open.
#[derive(Resource, Default)]
pub(crate) struct PickerOpen(pub bool);

/// Whether the scoreboard the last match ended on is open in the lobby.
#[derive(Resource, Default)]
pub(crate) struct FinalScoresOpen(pub bool);

/// The last match's scoreboard, over the lobby, and what is around it.
#[derive(Component)]
pub(super) struct FinalScoresPanel;

/// The button in the corner that shows the match and opens the picker.
#[derive(Component)]
pub(super) struct MatchButton;

/// The match picker, under the match button.
#[derive(Component)]
pub(super) struct MatchPicker;

/// Under the open match picker and over the rest of the lobby: a click on it
/// closes the picker.
#[derive(Component)]
pub(super) struct PickerCatcher;

/// The picture on the match button: the picked map's, out of every map's.
#[derive(Component)]
pub(super) struct MatchThumb(pub(super) Vec<Option<Handle<Image>>>);

/// How each air acceleration is shown, named after where it is played.
pub(super) fn air_accel_name(value: u16) -> String {
    match value {
        10 => "10 · CS".to_string(),
        100 => "100 · bhop".to_string(),
        other => other.to_string(),
    }
}

/// A team's half of the screen, faintly in its colour, brightest at the top.
/// (Colours mix in linear light, so a little alpha goes a long way over the
/// dark scrim.)
pub(super) fn team_tint(color: Color, left: bool) -> impl Bundle {
    let gradient = LinearGradient::to_bottom(vec![
        ColorStop::auto(color.with_alpha(0.035)),
        ColorStop::new(color.with_alpha(0.008), percent(70)),
    ]);
    (
        TeamModeOnly,
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            bottom: px(0),
            left: if left { px(0) } else { Val::Auto },
            right: if left { Val::Auto } else { px(0) },
            width: percent(50),
            ..default()
        },
        BackgroundGradient::from(gradient),
        Pickable::IGNORE,
    )
}

pub(super) fn spawn_lobby(mut commands: Commands, session: Res<Session>, asset_server: Res<AssetServer>) {
    commands.insert_resource(PickerOpen(false));
    commands.insert_resource(FinalScoresOpen(false));
    let share = session.share_address.clone();
    let server_label = session.server_label.clone();
    let pictures: Vec<_> = (0..crate::map::count()).map(|id| crate::browser::map_picture(&asset_server, id)).collect();
    let modes = (0..modes::count()).map(|mode| {
        (
            MatchChoice::Mode(mode),
            small_button(modes::name(mode)),
            Pickable::default(),
            observe(send_choice),
        )
    });
    let maps: Vec<_> = pictures.iter().enumerate().map(|(id, picture)| map_card(id as u8, picture.clone())).collect();
    let accels = AIR_ACCEL_CHOICES.map(|value| {
        (
            MatchChoice::AirAccel(value),
            small_button(&air_accel_name(value)),
            Pickable::default(),
            observe(send_choice),
        )
    });
    let bot_fire = [(false, "Shoot"), (true, "Hold fire")].map(|(hold, name)| {
        (
            MatchChoice::BotsHoldFire(hold),
            small_button(name),
            Pickable::default(),
            observe(send_choice),
        )
    });
    let bot_skill = (0..BOT_DIFFICULTY_NAMES.len() as u8).map(|level| {
        (
            MatchChoice::BotDifficulty(level),
            small_button(BOT_DIFFICULTY_NAMES[level as usize]),
            Pickable::default(),
            observe(send_choice),
        )
    });

    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                min_height: px(0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM.with_alpha(0.84)),
            DespawnOnExit(AppState::Lobby),
        ))
        .with_children(|screen| {
            screen.spawn(team_tint(BLUE, true));
            screen.spawn(team_tint(RED, false));
            screen.spawn((
                PickerCatcher,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    display: Display::None,
                    ..default()
                },
                GlobalZIndex(4),
                observe(|_: On<PointerClick>, mut picker: ResMut<PickerOpen>| picker.0 = false),
            ));
            screen.spawn(final_scores_panel());
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(1280),
                    height: percent(100),
                    // Never taller than the window: the player lines give way.
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(px(40), px(40), px(22), px(28)),
                    row_gap: px(18),
                    ..default()
                })
                .with_children(|page| {
                    page.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::FlexStart,
                        column_gap: px(18),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(Node::default()).with_children(|corner| {
                            corner.spawn(match_button(pictures.clone()));
                            corner
                                .spawn((
                                    MatchPicker,
                                    Node {
                                        display: Display::None,
                                        position_type: PositionType::Absolute,
                                        top: percent(100),
                                        left: px(0),
                                        width: px(560),
                                        margin: UiRect::top(px(10)),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(14),
                                        padding: UiRect::all(px(18)),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(18)),
                                        ..default()
                                    },
                                    GlobalZIndex(5),
                                    BackgroundColor(Color::srgb(0.102, 0.086, 0.075)),
                                    BorderColor::all(BUTTON_EDGE),
                                    soft_shadow(),
                                ))
                                .with_children(|picker| {
                                    picker.spawn((
                                        Node {
                                            justify_content: JustifyContent::SpaceBetween,
                                            align_items: AlignItems::Baseline,
                                            ..default()
                                        },
                                        children![
                                            heading("Match", 18.0, TEXT),
                                            (label("", 12.0, DIM), LobbyText::PickerNote),
                                        ],
                                    ));
                                    let group = || Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(8),
                                        min_width: px(0),
                                        ..default()
                                    };
                                    picker.spawn(group()).with_children(|group| {
                                        group.spawn(section("Mode"));
                                        group.spawn(Node {
                                            column_gap: px(6),
                                            row_gap: px(6),
                                            flex_wrap: FlexWrap::Wrap,
                                            ..default()
                                        })
                                        .with_children(|row| {
                                            for chip in modes {
                                                row.spawn(chip);
                                            }
                                        });
                                    });
                                    picker.spawn(group()).with_children(|group| {
                                        group.spawn(section("Map"));
                                        // A column for each map of the mode with the most,
                                        // so the cards keep their size when the maps the
                                        // mode isn't played on are hidden.
                                        let grid = Node {
                                            display: Display::Grid,
                                            grid_template_columns: RepeatedGridTrack::flex(crate::map::most_maps_in_a_mode() as u16, 1.0),
                                            column_gap: px(10),
                                            min_width: px(0),
                                            ..default()
                                        };
                                        group.spawn(grid).with_children(|row| {
                                            for card in maps {
                                                row.spawn(card);
                                            }
                                        });
                                    });
                                    picker.spawn(group()).with_children(|group| {
                                        group.spawn(section("Air accel"));
                                        group.spawn(Node { column_gap: px(6), ..default() }).with_children(|chips| {
                                            for chip in accels {
                                                chips.spawn(chip);
                                            }
                                        });
                                    });
                                    picker.spawn((group(), ArmedOnly)).with_children(|group| {
                                        group.spawn(section("Bots"));
                                        group.spawn(Node {
                                            column_gap: px(6),
                                            align_items: AlignItems::Center,
                                            ..default()
                                        })
                                        .with_children(|row| {
                                            row.spawn((
                                                Node {
                                                    align_self: AlignSelf::FlexStart,
                                                    align_items: AlignItems::Center,
                                                    column_gap: px(4),
                                                    padding: UiRect::all(px(3)),
                                                    border: UiRect::all(px(1)),
                                                    border_radius: BorderRadius::all(px(12)),
                                                    ..default()
                                                },
                                                BackgroundColor(FIELD),
                                                BorderColor::all(PANEL_EDGE),
                                                children![
                                                    leader_button("−", ClientMsg::RemoveBot),
                                                    (
                                                        weighted("", 16.0, 700, TEXT),
                                                        LobbyText::Bots,
                                                        TextLayout::justify(Justify::Center),
                                                        Node { min_width: px(34), ..default() },
                                                    ),
                                                    leader_button("+", ClientMsg::AddBot),
                                                ],
                                            ));
                                            for chip in bot_fire {
                                                row.spawn(chip);
                                            }
                                        });
                                    });
                                    picker.spawn((group(), ArmedOnly)).with_children(|group| {
                                        group.spawn(section("Bot skill"));
                                        group.spawn(Node { column_gap: px(6), ..default() }).with_children(|chips| {
                                            for chip in bot_skill {
                                                chips.spawn(chip);
                                            }
                                        });
                                    });
                                });
                        });
                        row.spawn((
                            ResultBanner,
                            Node {
                                display: Display::None,
                                align_items: AlignItems::Center,
                                column_gap: px(14),
                                padding: UiRect::new(px(16), px(6), px(6), px(6)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(12)),
                                ..default()
                            },
                            BackgroundColor(BLUE.with_alpha(0.12)),
                            BorderColor::all(BLUE.with_alpha(0.4)),
                            children![
                                (weighted("", 15.0, 700, TEXT), LobbyText::Result),
                                (
                                    small_button("Scoreboard"),
                                    observe(|_: On<Activate>, mut open: ResMut<FinalScoresOpen>| open.0 = !open.0),
                                ),
                            ],
                        ));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexEnd,
                            row_gap: px(8),
                            ..default()
                        })
                        .with_children(|side| {
                            side.spawn((weighted("", 12.0, 700, DIM), LobbyText::Caption));
                            match share {
                                // Hosting: how the others get in.
                                Some(address) => side.spawn(invite_box(address)),
                                None => side.spawn((
                                    Node {
                                        padding: UiRect::axes(px(16), px(10)),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(12)),
                                        ..default()
                                    },
                                    BackgroundColor(PANEL),
                                    BorderColor::all(PANEL_EDGE),
                                    children![label(format!("Connected to {server_label}"), 13.0, DIM)],
                                )),
                            };
                        });
                    });
                    // The players, in whatever room the corner and the bar leave.
                    page.spawn((
                        LobbyTeams::default(),
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_height: px(0),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                    ));
                    page.spawn((
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        },
                        children![
                            (danger_button("‹  Leave lobby"), observe(on_leave)),
                            (loadout_summary(), ArmedOnly),
                            (button("Paint yourself"), observe(on_open_paint)),
                            (button("Settings"), observe(on_open_settings)),
                            Node { flex_grow: 1.0, ..default() },
                            (
                                primary_button("Start match"),
                                LeaderOnly,
                                observe(send_on_click(ClientMsg::StartMatch)),
                            ),
                            (
                                Node {
                                    flex_shrink: 0.0,
                                    padding: UiRect::axes(px(22), px(16)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(14)),
                                    ..default()
                                },
                                BackgroundColor(PANEL),
                                BorderColor::all(PANEL_EDGE),
                                GuestOnly,
                                children![(weighted("", 16.0, 600, DIM), LobbyText::Waiting, TextLayout::no_wrap())],
                            ),
                        ],
                    ));
                });
        });
}

/// The scoreboard the last match ended on, over the whole lobby. A click
/// outside it, its button or Escape closes it.
pub(super) fn final_scores_panel() -> impl Bundle {
    (
        FinalScoresPanel,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        GlobalZIndex(6),
        Pickable::IGNORE,
        children![
            (
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(SCRIM.with_alpha(0.66)),
                observe(|_: On<PointerClick>, mut open: ResMut<FinalScoresOpen>| open.0 = false),
            ),
            (
                Node {
                    min_width: px(640),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
                    padding: UiRect::axes(px(28), px(26)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(26)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(PANEL_EDGE),
                soft_shadow(),
                children![
                    (
                        Node {
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::FlexEnd,
                            column_gap: px(24),
                            margin: UiRect::bottom(px(6)),
                            ..default()
                        },
                        children![
                            (
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(6),
                                    ..default()
                                },
                                children![
                                    (weighted("", 12.0, 700, DIM), LobbyText::FinalCaption),
                                    heading("Final scores", 30.0, TEXT),
                                ],
                            ),
                            (
                                small_button("Close"),
                                observe(|_: On<Activate>, mut open: ResMut<FinalScoresOpen>| open.0 = false),
                            ),
                        ],
                    ),
                    roster_list(ROSTER_WIDTH, true),
                ],
            ),
        ],
    )
}

/// The button in the corner: the map's picture, the mode, the map. It opens
/// the match picker, for the leader to change it and the others to look.
pub(super) fn match_button(pictures: Vec<Option<Handle<Image>>>) -> impl Bundle {
    let kind = ButtonKind::Tile;
    (
        MatchButton,
        Button,
        kind,
        Hovered::default(),
        Node {
            align_items: AlignItems::Center,
            column_gap: px(14),
            padding: UiRect::new(px(6), px(20), px(6), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(kind.colors()[0]),
        BorderColor::all(kind.edge()),
        observe(|_: On<Activate>, mut picker: ResMut<PickerOpen>| picker.0 = !picker.0),
        children![
            (
                MatchThumb(pictures),
                ImageNode::default(),
                Node {
                    width: px(112),
                    height: px(63),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (weighted("", 10.0, 700, DIM), LobbyText::MatchHint, Pickable::IGNORE),
                    (heading("", 20.0, ACCENT), LobbyText::Mode, Pickable::IGNORE),
                    (weighted("", 14.0, 600, TEXT), LobbyText::MatchLine, Pickable::IGNORE),
                ],
            ),
        ],
    )
}

/// The address friends join with, and a button that copies it.
pub(super) fn invite_box(address: String) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::new(px(16), px(5), px(5), px(5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_EDGE),
        children![
            label("Friends join with", 13.0, DIM),
            weighted(address.clone(), 16.0, 700, TEXT),
            (
                button_box(ButtonKind::Normal, true),
                children![(weighted("Copy", 14.0, 600, TEXT), Pickable::IGNORE, CopyLabel)],
                observe(
                    move |_: On<Activate>, mut clipboard: ResMut<Clipboard>, mut text: Single<&mut Text, With<CopyLabel>>| {
                        let done = clipboard.set_text(address.as_str()).is_ok();
                        text.0 = if done { "Copied" } else { "Failed" }.to_string();
                    },
                ),
            ),
        ],
    )
}

/// A map to pick: its picture, its name, and a note under it.
pub(super) fn map_card(id: u8, picture: Option<Handle<Image>>) -> impl Bundle {
    let choice = MatchChoice::Map(id);
    (
        choice,
        Button,
        ButtonKind::Tile,
        Hovered::default(),
        Pickable::default(),
        Node {
            // Not held as wide as its picture.
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(ButtonKind::Tile.edge()),
        observe(send_choice),
        children![
            (
                picture.map(ImageNode::new).unwrap_or_default(),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    aspect_ratio: Some(16.0 / 9.0),
                    // The card clips square, so the picture rounds its own top
                    // corners to the card's, inside its 1px edge.
                    border_radius: BorderRadius::top(px(11)),
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::axes(px(10), px(7)),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (weighted(crate::map::name(id), 14.0, 700, TEXT), Pickable::IGNORE),
                    (MapNote(id), label("", 11.0, DIM), Pickable::IGNORE),
                ],
            ),
        ],
    )
}

pub(super) fn leader_button(text: &str, msg: ClientMsg) -> impl Bundle + use<> {
    (small_button(text), LeaderOnly, observe(send_on_click(msg)))
}

/// Sends the leader's pick of a mode, map, air acceleration or bots' fire.
pub(super) fn send_choice(
    activate: On<Activate>,
    choices: Query<&MatchChoice>,
    session: Res<Session>,
    connection: Option<ResMut<Connection>>,
) {
    let (Ok(choice), Some(mut connection)) = (choices.get(activate.entity), connection) else {
        return;
    };
    if !session.is_leader() {
        return;
    }
    connection.send(&match *choice {
        MatchChoice::Mode(mode) => ClientMsg::SetMode(mode),
        MatchChoice::Map(id) => ClientMsg::SetMap(id),
        MatchChoice::AirAccel(value) => ClientMsg::SetAirAccel(value),
        MatchChoice::BotsHoldFire(hold) => ClientMsg::SetBotsHoldFire(hold),
        MatchChoice::BotDifficulty(level) => ClientMsg::SetBotDifficulty(level),
    });
}

/// An observer that sends `msg` to the server when its button is clicked.
pub(super) fn send_on_click(msg: ClientMsg) -> impl Fn(On<Activate>, Option<ResMut<Connection>>) {
    move |_, connection| {
        if let Some(mut connection) = connection {
            connection.send(&msg);
        }
    }
}

pub(super) fn update_lobby(
    session: Res<Session>,
    map: Res<GameMap>,
    keys: Res<ButtonInput<KeyCode>>,
    mut picker: ResMut<PickerOpen>,
    mut final_scores: ResMut<FinalScoresOpen>,
    mut texts: Query<(&LobbyText, &mut Text)>,
    mut thumbs: Query<(&MatchThumb, &mut ImageNode)>,
    mut match_button: Query<&mut BorderColor, With<MatchButton>>,
    mut shown: ParamSet<(
        Query<&mut Node, With<LeaderOnly>>,
        Query<&mut Node, With<GuestOnly>>,
        Query<&mut Node, With<TeamModeOnly>>,
        Query<&mut Node, With<ResultBanner>>,
        Query<&mut Node, Or<(With<MatchPicker>, With<PickerCatcher>)>>,
        Query<&mut Node, With<FinalScoresPanel>>,
        Query<&mut Node, With<ArmedOnly>>,
    )>,
) {
    if picker.0 && keys.just_pressed(KeyCode::Escape) {
        picker.0 = false;
    }
    if final_scores.0 && (keys.just_pressed(KeyCode::Escape) || session.last_match.is_none()) {
        final_scores.0 = false;
    }
    let leader = session.name_of(session.leader).to_string();
    let map_name = crate::map::name(map.0.id);
    for (kind, mut text) in &mut texts {
        let value = match kind {
            LobbyText::Caption if session.is_leader() => "YOUR LOBBY  ·  YOU PICK THE MATCH".to_string(),
            LobbyText::Caption => format!("{leader}'S LOBBY  ·  {leader} PICKS THE MATCH").to_uppercase(),
            LobbyText::MatchHint if session.is_leader() => "MATCH  ·  CLICK TO CHANGE".to_string(),
            LobbyText::MatchHint => "MATCH  ·  CLICK TO SEE THE CHOICES".to_string(),
            LobbyText::Mode => modes::name(session.mode).to_uppercase(),
            LobbyText::MatchLine if is_course_mode(session.mode) => {
                format!("{map_name}  ·  air accel {}  ·  no guns, no bots", session.air_accel() as u16)
            }
            LobbyText::MatchLine => {
                let bots = if session.bots_hold_fire { "  ·  bots hold fire" } else { "" };
                // Only when there are bots to be easy or hard.
                let skill = match BOT_DIFFICULTY_NAMES.get(session.bot_difficulty as usize) {
                    Some(name) if session.roster.iter().any(|player| player.is_bot) => {
                        format!("  ·  {} bots", name.to_lowercase())
                    }
                    _ => String::new(),
                };
                format!("{map_name}  ·  air accel {}{skill}{bots}", session.air_accel() as u16)
            }
            LobbyText::PickerNote if session.is_leader() => "Everyone sees your changes".to_string(),
            LobbyText::PickerNote => format!("Only {leader} can change these"),
            LobbyText::Bots => session.roster.iter().filter(|player| player.is_bot).count().to_string(),
            LobbyText::Result => session.last_result.clone().unwrap_or_default(),
            LobbyText::FinalCaption => match &session.last_match {
                Some(last) => {
                    let map = crate::map::name(last.map);
                    let mode = modes::name(last.mode);
                    format!("LAST MATCH  ·  {map}  ·  {mode}").to_uppercase()
                }
                None => String::new(),
            },
            LobbyText::Waiting => format!("Waiting for {leader} to start the match"),
        };
        set_text(&mut text, value);
    }
    for (thumb, mut image) in &mut thumbs {
        if let Some(Some(picture)) = thumb.0.get(map.0.id as usize)
            && image.image != *picture
        {
            image.image = picture.clone();
        }
    }
    let edge = if picker.0 { ACCENT } else { ButtonKind::Tile.edge() };
    for mut border in &mut match_button {
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
    }
    for mut node in &mut shown.p0() {
        set_shown(&mut node, session.is_leader());
    }
    for mut node in &mut shown.p1() {
        set_shown(&mut node, !session.is_leader());
    }
    for mut node in &mut shown.p2() {
        set_shown(&mut node, session.team_mode());
    }
    for mut node in &mut shown.p3() {
        set_shown(&mut node, session.last_result.is_some());
    }
    for mut node in &mut shown.p4() {
        set_shown(&mut node, picker.0);
    }
    for mut node in &mut shown.p5() {
        set_shown(&mut node, final_scores.0);
    }
    for mut node in &mut shown.p6() {
        set_shown(&mut node, modes::info(session.mode).armed);
    }
}

/// Marks the mode, map, air acceleration and bots' fire picked, hides the maps the mode
/// isn't played on, and lets only the leader click any of them.
pub(super) fn style_match_choices(
    session: Res<Session>,
    map: Res<GameMap>,
    mut choices: Query<(&MatchChoice, &mut ButtonKind, &mut BorderColor, &mut Pickable, &mut Node)>,
    mut notes: Query<(&MapNote, &mut Text)>,
) {
    let air_accel = session.air_accel() as u16;
    for (choice, mut kind, mut border, mut pickable, mut node) in &mut choices {
        let (picked, playable, rest) = match *choice {
            MatchChoice::Mode(mode) => (session.mode == mode, true, ButtonKind::Normal),
            MatchChoice::Map(id) => (map.0.id == id, crate::map::plays(id, session.mode), ButtonKind::Tile),
            MatchChoice::AirAccel(value) => (air_accel == value, true, ButtonKind::Normal),
            MatchChoice::BotsHoldFire(hold) => (session.bots_hold_fire == hold, true, ButtonKind::Normal),
            MatchChoice::BotDifficulty(level) => (session.bot_difficulty == level, true, ButtonKind::Normal),
        };
        let wanted = if picked { ButtonKind::Picked } else { rest };
        if *kind != wanted {
            *kind = wanted;
            *border = BorderColor::all(wanted.edge());
        }
        let clickable = session.is_leader() && playable;
        if pickable.is_hoverable != clickable {
            pickable.is_hoverable = clickable;
        }
        if let MatchChoice::Map(_) = *choice {
            set_shown(&mut node, playable);
        }
    }
    for (note, mut text) in &mut notes {
        let line = if map.0.id == note.0 { "Picked" } else { "" };
        set_text(&mut text, line.to_string());
    }
}

/// The lobby's players, with a line for every slot, rebuilt by
/// `update_lobby_teams` when anything on them changes.
#[derive(Component, Default)]
pub(super) struct LobbyTeams {
    /// What the lines were last built from.
    pub(super) shown: Option<LobbyKey>,
}

/// Everything the lobby's player lines show.
#[derive(Clone, PartialEq)]
pub(super) struct LobbyKey {
    /// In the order the server lists them.
    pub(super) roster: Vec<PlayerInfo>,
    /// Each player's main paint colour, in roster order.
    pub(super) colors: Vec<u8>,
    pub(super) my_id: PlayerId,
    pub(super) leader: PlayerId,
    pub(super) team_mode: bool,
    pub(super) my_team: u8,
}

/// An open slot on the other team, in its colour: a click puts us there.
#[derive(Component)]
pub(super) struct JoinSlot(pub(super) Color);

/// How high a player's line is at most.
pub(super) const LINE_HEIGHT: f32 = 52.0;

pub(super) fn update_lobby_teams(
    mut commands: Commands,
    session: Res<Session>,
    skins: Res<Skins>,
    my_paint: Option<Res<crate::paint::MyPaint>>,
    mut lists: Query<(Entity, &mut LobbyTeams)>,
) {
    let Ok((entity, mut list)) = lists.single_mut() else {
        return;
    };
    let colors = session
        .roster
        .iter()
        .map(|player| match &my_paint {
            Some(mine) if player.id == session.my_id => mine.canvas.main_color(),
            _ => skins.main_color(player.id).unwrap_or(player.color),
        })
        .collect();
    let key = LobbyKey {
        roster: session.roster.clone(),
        colors,
        my_id: session.my_id,
        leader: session.leader,
        team_mode: session.team_mode(),
        my_team: session.team_of(session.my_id),
    };
    if list.shown.as_ref() == Some(&key) {
        return;
    }
    list.shown = Some(key.clone());
    let players = |team: Option<u8>| -> Vec<usize> {
        (0..key.roster.len()).filter(|&i| team.is_none_or(|team| key.roster[i].team & 1 == team)).collect()
    };
    commands.entity(entity).despawn_children().with_children(|content| {
        if !key.team_mode {
            let everyone = players(None);
            content.spawn(team_heading("Free for all", everyone.len(), MAX_PLAYERS, TEXT, false));
            content
                .spawn(Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::flex(2, 1.0),
                    grid_template_rows: RepeatedGridTrack::flex((MAX_PLAYERS / 2) as u16, 1.0),
                    grid_auto_flow: GridAutoFlow::Column,
                    column_gap: px(96),
                    ..default()
                })
                .with_children(|grid| {
                    for slot in 0..MAX_PLAYERS.max(everyone.len()) {
                        spawn_slot(grid, &key, everyone.get(slot).copied(), slot, None, false);
                    }
                });
            return;
        }
        content
            .spawn(Node {
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            })
            .with_children(|row| {
                // Our team (blue) on the left, the enemy (red) facing it on
                // the right. The other team's open slots take a click.
                let mine = key.my_team & 1;
                for team in [mine, mine ^ 1] {
                    let color = team_color(mine, team);
                    let right = team != mine;
                    if right {
                        row.spawn(team_seam());
                    }
                    let on_team = players(Some(team));
                    let join = (team != key.my_team & 1 && on_team.len() < TEAM_SIZE).then_some(color);
                    let title = format!("{} team", team_name(mine, team));
                    row.spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_width: px(0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn(team_heading(&title, on_team.len(), TEAM_SIZE, color, right));
                        for slot in 0..TEAM_SIZE.max(on_team.len()) {
                            spawn_slot(column, &key, on_team.get(slot).copied(), slot, join, right);
                        }
                    });
                }
            });
    });
}

/// A team's name, big, and how many of its slots are taken.
pub(super) fn team_heading(title: &str, count: usize, slots: usize, color: Color, right: bool) -> impl Bundle + use<> {
    let count = (weighted(format!("{count} / {slots}"), 15.0, 700, DIM), TextLayout::no_wrap());
    (
        Node {
            align_items: AlignItems::Baseline,
            justify_content: if right { JustifyContent::FlexEnd } else { JustifyContent::FlexStart },
            flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row },
            column_gap: px(14),
            margin: UiRect::bottom(px(8)),
            ..default()
        },
        children![(heading(title, 40.0, color), TextLayout::no_wrap()), count],
    )
}

/// Between the two teams: a thin line with VS in the middle.
pub(super) fn team_seam() -> impl Bundle {
    let line = || {
        (
            Node {
                width: px(1),
                flex_grow: 1.0,
                ..default()
            },
            BackgroundColor(PANEL_EDGE),
        )
    };
    (
        Node {
            width: px(96),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: UiRect::vertical(px(12)),
            ..default()
        },
        children![
            line(),
            (
                Node {
                    width: px(58),
                    height: px(58),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(29)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(BUTTON_EDGE),
                children![heading("vs", 17.0, DIM)],
            ),
            line(),
        ],
    )
}

/// One line of a team, or of free for all: slot `slot`, with the player at
/// `index` in the roster, or open. An open slot on the other team, `join`,
/// takes a click. The right-hand team's lines run from the right.
pub(super) fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    key: &LobbyKey,
    index: Option<usize>,
    slot: usize,
    join: Option<Color>,
    right: bool,
) {
    let line = Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_height: px(0),
        max_height: px(LINE_HEIGHT),
        align_items: AlignItems::Center,
        flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row },
        column_gap: px(14),
        padding: UiRect::axes(px(6), px(0)),
        border: UiRect::bottom(px(1)),
        ..default()
    };
    let number = (
        label((slot + 1).to_string(), 13.0, FAINT),
        TextLayout::justify(if right { Justify::Right } else { Justify::Left }),
        Node { width: px(20), ..default() },
        Pickable::IGNORE,
    );
    let dot = |color: Color, edge: Color| {
        (
            Node {
                width: px(20),
                height: px(20),
                flex_shrink: 0.0,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(edge),
            Pickable::IGNORE,
        )
    };
    let Some(index) = index else {
        let (text, ink) = match join {
            Some(color) => ("Open  ·  join this team  ›", color),
            None => ("Open", FAINT),
        };
        let mut open = parent.spawn((
            line,
            BorderColor::all(PANEL_EDGE),
            BackgroundColor(Color::NONE),
            children![
                number,
                dot(Color::NONE, FAINT.with_alpha(0.6)),
                (weighted(text, 16.0, if join.is_some() { 700 } else { 500 }, ink), TextLayout::no_wrap(), Pickable::IGNORE),
            ],
        ));
        if let Some(color) = join {
            open.insert((JoinSlot(color), Hovered::default(), observe(on_join_click)));
        }
        return;
    };
    let player = &key.roster[index];
    let me = player.id == key.my_id;
    let (name, tag) = match player.name.strip_prefix("[bot] ") {
        Some(name) if player.is_bot => (name, "BOT"),
        _ if player.id == key.leader => (player.name.as_str(), "LEADER"),
        _ => (player.name.as_str(), ""),
    };
    let (tag_ink, tag_fill, tag_edge) = if tag == "LEADER" {
        (ACCENT, ACCENT.with_alpha(0.08), ACCENT.with_alpha(0.4))
    } else {
        (DIM, CARD, PANEL_EDGE)
    };
    parent.spawn((
        line,
        BorderColor::all(PANEL_EDGE),
        children![
            number,
            dot(palette_color(key.colors[index]), Color::NONE),
            (
                weighted(name.to_string(), 23.0, if me { 800 } else { 600 }, if me { ACCENT } else { TEXT }),
                TextLayout::no_wrap(),
            ),
            tag_chip(if me { "YOU" } else { "" }, ON_PRIMARY, ACCENT, Color::NONE),
            tag_chip(tag, tag_ink, tag_fill, tag_edge),
        ],
    ));
}

/// A small label beside a name; nothing at all if `text` is empty.
pub(super) fn tag_chip(text: &str, ink: Color, fill: Color, edge: Color) -> impl Bundle + use<> {
    (
        Node {
            display: if text.is_empty() { Display::None } else { Display::Flex },
            padding: UiRect::axes(px(7), px(3)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(edge),
        children![weighted(text.to_string(), 10.0, 800, ink)],
    )
}

/// A click on an open slot of the other team moves us onto it.
pub(super) fn on_join_click(click: On<PointerClick>, connection: Option<ResMut<Connection>>) {
    if click.button == PointerButton::Primary
        && let Some(mut connection) = connection
    {
        connection.send(&ClientMsg::SwitchTeam);
    }
}

/// An open slot we can join lights up in its team's colour under the mouse.
pub(super) fn style_join_slots(mut slots: Query<(&JoinSlot, &Hovered, &mut BackgroundColor)>) {
    for (slot, hovered, mut background) in &mut slots {
        let wanted = if hovered.get() { slot.0.with_alpha(0.08) } else { Color::NONE };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

//! What is on screen during a match: the HUD, the scoreboard and pause
//! panels over it, and the death screen.

use super::*;

/// The pieces of text on the HUD.
#[derive(Component, Clone, Copy)]
pub(super) enum Hud {
    Clock,
    /// The caption over one of the two scores beside the clock (0 = left).
    ScoreLabel(usize),
    Score(usize),
    /// The line under the clock: score limit, team, hill.
    Info,
    /// Frames per second, over the ping.
    Fps,
    Ping,
    Speed,
    DashLabel,
    /// One piece of one line of the kill feed.
    Feed(usize, FeedPart),
    /// "Get ready" before our first spawn.
    Center,
    /// On the death screen: who killed us.
    Killer,
    /// Under it: how many kills that makes for them.
    KillerNote,
    /// The teammate we see through while we are dead: their name, health,
    /// armor, and the gun in their hands.
    Watching,
    WatchedHealth,
    WatchedArmor,
    WatchedGun,
    /// The seconds until we respawn.
    RespawnIn,
    /// How to respawn now.
    RespawnHint,
    /// A tip, a different one each death.
    Tip,
    /// The pause menu's lines: the time left, the map and mode, and how we are doing.
    PauseClock,
    PauseInfo,
    PausePlace,
    PauseKills,
    PauseAssists,
    PauseDeaths,
    PauseDamage,
    /// The scoreboard's title: the map and what wins, the mode, and the time left.
    ScoreCaption,
    ScoreMode,
    ScoreClock,
    /// Under the crosshair: the grenade in hand, and what a click does with it
    /// (or a cooking frag's fuse).
    Hand,
    /// "FRAGGED somebody".
    Kill,
    /// Streaks and revenge, under the kill line.
    Banner,
    /// The seconds left before the match starts, then "GO".
    Countdown,
    /// The line over the countdown.
    CountdownLabel,
    /// The racing modes: our run's clock, and the checkpoints under it.
    RunClock,
    RunInfo,
    /// The pause menu in the racing modes: our time, and checkpoints.
    PauseTime,
    PauseGates,
}

/// The part of a bar that grows and shrinks.
#[derive(Component, Clone, Copy)]
pub(super) enum Bar {
    Health,
    /// Trails behind the health bar, showing what was just lost.
    HealthLag,
    Armor,
    Dash,
    /// The teammate we see through while dead.
    WatchedHealth,
    WatchedArmor,
}

/// The box around one line of the kill feed.
#[derive(Component)]
pub(super) struct FeedRow(pub(super) usize);

/// The pieces of a kill feed line, left to right: killer, weapon picture,
/// victim, and what follows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FeedPart {
    Killer,
    Victim,
    Tail,
}

/// The weapon picture of a kill feed line.
#[derive(Component)]
pub(super) struct FeedWeapon(pub(super) usize);

/// Size of a weapon picture in the kill feed (the pictures are 640 by 240).
pub(super) const FEED_PICTURE: Vec2 = Vec2::new(64.0, 24.0);

/// The coloured shapes on the HUD.
#[derive(Component, Clone, Copy)]
pub(super) enum Overlay {
    /// Red flash over the whole screen when hurt.
    DamageFlash,
    /// White over the whole screen while blinded by a flashbang.
    Blind,
    /// Grey over the whole screen while inside a smoke cloud.
    Smoke,
    /// One of the four ticks that appear around the crosshair on a hit.
    HitTick,
}

#[derive(Component)]
pub(super) struct ScoreboardPanel;
#[derive(Component)]
pub(super) struct PausePanel;

/// Something pinned to part of the screen during a match. Never blocks mouse clicks.
pub(super) fn pinned(node: Node) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        Pickable::IGNORE,
        DespawnOnExit(AppState::Playing),
    )
}

/// A HUD element pinned to part of the screen, sized by the player's "HUD size".
pub(super) fn hud_anchor(node: Node) -> impl Bundle {
    (pinned(node), HudScaled)
}

/// A full-screen HUD layer that centres its content. Not sized by "HUD size":
/// what is in it lines up with the view (the crosshair, the choke ring).
pub(super) fn hud_layer() -> impl Bundle {
    pinned(Node {
        width: percent(100),
        height: percent(100),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    })
}

/// A line across the screen at `top` percent of its height, text centred.
pub(super) fn hud_line(top: f32) -> impl Bundle {
    hud_anchor(Node {
        width: percent(100),
        top: percent(top),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: px(4),
        ..default()
    })
}

/// HUD text, with a shadow so it reads against any background.
pub(super) fn hud_text(size: f32, color: Color, hud: Hud) -> impl Bundle {
    (label("", size, color), hud_shadow(), hud)
}

pub fn hud_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::splat(1.5),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
    }
}

/// A dark see-through box with a faint edge.
pub(super) fn hud_panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..node
        },
        BackgroundColor(HUD_PANEL),
        BorderColor::all(HUD_EDGE),
    )
}

/// The empty part of a bar; give it `bar_fill` children.
pub(super) fn bar_track(width: Val, height: f32) -> impl Bundle {
    (
        Node {
            width,
            height: px(height),
            border_radius: BorderRadius::all(px(height / 2.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(TRACK),
    )
}

pub(super) fn bar_fill(bar: Bar, color: Color) -> impl Bundle {
    (
        bar,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(0),
            height: percent(100),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(color),
    )
}

/// One of the two scores beside the clock, under its caption.
pub(super) fn score_block(side: usize) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            min_width: px(96),
            ..default()
        },
        children![
            hud_text(12.0, DIM, Hud::ScoreLabel(side)),
            hud_text(26.0, TEXT, Hud::Score(side)),
        ],
    )
}

/// The frame rate and ping box.
#[derive(Component)]
pub(super) struct StatsBox;

pub(super) fn show_stats(settings: Res<Settings>, mut boxes: Query<&mut Node, With<StatsBox>>) {
    for mut node in &mut boxes {
        set_shown(&mut node, settings.graphics.show_stats);
    }
}

/// One line of the frame rate and ping box: a caption on the left, the value
/// lined up on the right so it does not wobble as the digits change.
pub(super) fn stat_row(caption: &str, hud: Hud) -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        },
        children![
            (label(caption, 11.0, DIM), hud_shadow()),
            (
                Node {
                    min_width: px(48),
                    justify_content: JustifyContent::End,
                    ..default()
                },
                children![hud_text(14.0, TEXT, hud)],
            ),
        ],
    )
}

/// One line of the kill feed (it holds seven at most).
pub(super) fn feed_row(row: usize) -> impl Bundle {
    (
        FeedRow(row),
        Node {
            padding: UiRect::axes(px(10), px(3)),
            border_radius: BorderRadius::all(px(5)),
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
        BackgroundColor(HUD_PANEL),
        children![
            hud_text(16.0, TEXT, Hud::Feed(row, FeedPart::Killer)),
            (
                FeedWeapon(row),
                Node {
                    width: px(FEED_PICTURE.x),
                    height: px(FEED_PICTURE.y),
                    margin: UiRect::horizontal(px(8)),
                    display: Display::None,
                    ..default()
                },
                ImageNode::default(),
            ),
            hud_text(16.0, TEXT, Hud::Feed(row, FeedPart::Victim)),
            hud_text(16.0, TEXT, Hud::Feed(row, FeedPart::Tail)),
        ],
    )
}

/// One tick of the hit marker: a short diagonal stroke `(dx, dy)` away from
/// the middle of its 40 x 40 box.
pub(super) fn hit_tick(dx: f32, dy: f32) -> impl Bundle {
    const LENGTH: f32 = 12.0;
    const THICKNESS: f32 = 3.0;
    // The strokes point at the crosshair: "\" on one diagonal, "/" on the other.
    let angle = if dx * dy > 0.0 { 45.0 } else { -45.0 };
    (
        Overlay::HitTick,
        Node {
            position_type: PositionType::Absolute,
            left: px(20.0 + dx - LENGTH / 2.0),
            top: px(20.0 + dy - THICKNESS / 2.0),
            width: px(LENGTH),
            height: px(THICKNESS),
            ..default()
        },
        UiTransform::from_rotation(Rot2::degrees(angle)),
        BackgroundColor(Color::NONE),
    )
}

pub(super) fn spawn_hud(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // The view through a scope, under the rest of the HUD: its lens is a
    // square as tall as the screen, black bars fill the sides.
    let lens = images.add(scope_lens_image());
    let side = || {
        (
            ScopeSide,
            Node {
                flex_grow: 1.0,
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
        )
    };
    commands.spawn((
        ScopeView,
        hud_anchor(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Row,
            ..default()
        }),
        Visibility::Hidden,
        children![
            side(),
            (
                ScopeLens,
                Node {
                    height: percent(100),
                    aspect_ratio: Some(1.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(lens),
                Pickable::IGNORE,
            ),
            side(),
        ],
    ));

    commands.spawn((hud_layer(), Overlay::DamageFlash, BackgroundColor(Color::NONE)));
    commands.spawn((hud_layer(), Overlay::Smoke, BackgroundColor(Color::NONE)));
    commands.spawn((hud_layer(), Overlay::Blind, BackgroundColor(Color::NONE)));

    // Crosshair: a white dot with a dark rim, so it shows against anything.
    commands.spawn((
        Crosshair,
        hud_layer(),
        children![(
            Node {
                width: px(8),
                height: px(8),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::WHITE),
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        )],
    ));

    // A gun with a choke, aimed: a ring around the crosshair as wide as its spread.
    commands.spawn((
        hud_layer(),
        children![(
            ChokeRing,
            Node {
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BorderColor::all(Color::NONE),
            Visibility::Hidden,
        )],
    ));

    // Hit marker, around the crosshair.
    commands.spawn((
        hud_layer(),
        children![(
            Node {
                width: px(40),
                height: px(40),
                ..default()
            },
            children![
                hit_tick(-11.0, -11.0),
                hit_tick(11.0, -11.0),
                hit_tick(-11.0, 11.0),
                hit_tick(11.0, 11.0),
            ],
        )],
    ));

    // Where hits come from: arcs on a ring around the crosshair.
    let arc = images.add(damage_arc_image());
    let arcs = (0..DAMAGE_ARCS).map(move |index| {
        (
            DamageArcNode(index),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode::new(arc.clone()).with_color(Color::NONE),
            UiTransform::default(),
            Pickable::IGNORE,
        )
    });
    commands.spawn((
        hud_layer(),
        children![(
            Node {
                width: px(DAMAGE_RING),
                height: px(DAMAGE_RING),
                ..default()
            },
            Pickable::IGNORE,
            Children::spawn(SpawnIter(arcs)),
        )],
    ));

    // The grenade in hand, just under the crosshair.
    commands.spawn((hud_line(55.0), children![hud_text(18.0, ACCENT, Hud::Hand)]));

    // The countdown before the match, big, above the crosshair.
    commands.spawn((
        hud_line(26.0),
        children![
            hud_text(20.0, TEXT, Hud::CountdownLabel),
            hud_text(96.0, ACCENT, Hud::Countdown),
        ],
    ));

    // Before our first spawn, a bit above the middle of the screen.
    commands.spawn((hud_line(30.0), children![hud_text(30.0, DANGER, Hud::Center)]));

    // The racing modes: our run's clock, low in the middle.
    commands.spawn((
        hud_line(74.0),
        children![hud_text(40.0, TEXT, Hud::RunClock), hud_text(15.0, DIM, Hud::RunInfo)],
    ));

    // Kill announcements, a bit below it.
    commands.spawn((
        hud_line(60.0),
        children![hud_text(28.0, TEXT, Hud::Kill), hud_text(22.0, ACCENT, Hud::Banner)],
    ));

    // Top middle: the clock between the two scores, and a line of detail under it.
    commands.spawn((
        hud_anchor(Node {
            width: percent(100),
            top: px(12),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        }),
        children![
            (
                hud_panel(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    padding: UiRect::axes(px(14), px(6)),
                    ..default()
                }),
                children![score_block(0), hud_text(32.0, TEXT, Hud::Clock), score_block(1)],
            ),
            hud_text(14.0, DIM, Hud::Info),
        ],
    ));

    // Frame rate and ping, top right, unless hidden in the graphics settings.
    commands.spawn((
        StatsBox,
        hud_anchor(Node {
            right: px(20),
            top: px(14),
            ..default()
        }),
        children![(
            hud_panel(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                padding: UiRect::axes(px(10), px(5)),
                ..default()
            }),
            children![stat_row("FPS", Hud::Fps), stat_row("PING", Hud::Ping)],
        )],
    ));

    // Kill feed, top right. Kept on the death screen, over its tint.
    commands.spawn((
        GlobalZIndex(1),
        hud_anchor(Node {
            right: px(20),
            top: px(68),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(4),
            ..default()
        }),
        children![
            feed_row(0),
            feed_row(1),
            feed_row(2),
            feed_row(3),
            feed_row(4),
            feed_row(5),
            feed_row(6),
        ],
    ));

    // Health, bottom left. Hidden on the death screen, like the speed and the
    // weapon panel, which would be in the way of its buttons; and on a
    // course, where nobody is hurt.
    commands.spawn((
        HiddenWhenDead,
        ArmedOnly,
        hud_anchor(Node {
            left: px(20),
            bottom: px(18),
            ..default()
        }),
        children![(
            hud_panel(Node {
                width: px(280),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::axes(px(14), px(10)),
                ..default()
            }),
            children![
                // Armor on top: the bar is full at the most you can carry.
                (bar_track(percent(100), 6.0), children![bar_fill(Bar::Armor, ARMOR)]),
                (
                    bar_track(percent(100), 10.0),
                    children![
                        bar_fill(Bar::HealthLag, Color::srgba(1.0, 1.0, 1.0, 0.75)),
                        bar_fill(Bar::Health, GOOD),
                    ],
                ),
            ],
        )],
    ));

    // Speed and dash, bottom middle. Hidden on the death screen, which
    // reaches down to it.
    commands.spawn((
        HiddenWhenDead,
        hud_anchor(Node {
            width: percent(100),
            bottom: px(22),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        }),
        children![
            hud_text(20.0, TEXT, Hud::Speed),
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                },
                children![
                    hud_text(13.0, DIM, Hud::DashLabel),
                    (bar_track(px(120), 6.0), children![bar_fill(Bar::Dash, ACCENT)]),
                ],
            ),
        ],
    ));

    // The death screen, around the edges so the middle is clear for the
    // teammate we see through: who got us (bottom left, over what we will
    // respawn with), the teammate (bottom middle), when we respawn (bottom
    // right) and, in conquest, the map to pick where (top left). The mouse
    // is free: a click on the view steps to another teammate, and the
    // loadout opens in a panel over it all.
    commands.spawn((
        DeathScreen,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::None,
            ..default()
        },
        DespawnOnExit(AppState::Playing),
        observe(on_death_screen_click),
        children![
            crate::minimap::deploy_map(),
            // Bottom left: who killed us, and the next life.
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(24),
                    bottom: px(22),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Start,
                    row_gap: px(14),
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            (weighted("FRAGGED BY", 12.0, 700, DANGER_TEXT), hud_shadow(), Pickable::IGNORE),
                            (heading("", 34.0, RED), Hud::Killer, hud_shadow(), Pickable::IGNORE),
                            (label("", 15.0, TEXT), Hud::KillerNote, hud_shadow(), Pickable::IGNORE),
                        ],
                    ),
                    (
                        Node {
                            border_radius: BorderRadius::all(px(12)),
                            max_width: px(420),
                            ..default()
                        },
                        BackgroundColor(PANEL.with_alpha(0.85)),
                        children![(
                            summary_button("NEXT LIFE", "L  ·  change loadout"),
                            DeathLoadoutSummary,
                            observe(|_: On<Activate>, mut open: ResMut<DeathLoadoutOpen>| open.0 = true),
                        )],
                    ),
                ],
            ),
            // Bottom middle: the teammate we see through.
            (
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    bottom: px(26),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (
                        WatchPanel,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(6),
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            (heading("", 26.0, BLUE), Hud::Watching, hud_shadow(), Pickable::IGNORE),
                            (
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    ..default()
                                },
                                Pickable::IGNORE,
                                children![
                                    (
                                        Node {
                                            flex_direction: FlexDirection::Column,
                                            row_gap: px(4),
                                            ..default()
                                        },
                                        Pickable::IGNORE,
                                        children![
                                            (bar_track(px(240), 5.0), Pickable::IGNORE, children![bar_fill(Bar::WatchedArmor, ARMOR)]),
                                            (bar_track(px(240), 9.0), Pickable::IGNORE, children![bar_fill(Bar::WatchedHealth, GOOD)]),
                                        ],
                                    ),
                                    (weighted("", 16.0, 700, TEXT), Hud::WatchedHealth, hud_shadow(), Pickable::IGNORE),
                                    (weighted("", 13.0, 700, ARMOR), Hud::WatchedArmor, hud_shadow(), Pickable::IGNORE),
                                ],
                            ),
                            (label("", 14.0, TEXT), Hud::WatchedGun, hud_shadow(), Pickable::IGNORE),
                            (label("click: next  ·  right-click: back", 12.0, DIM), hud_shadow(), Pickable::IGNORE),
                        ],
                    ),
                    (DeathHint::Alone, label("No teammates alive", 15.0, DIM), hud_shadow(), Pickable::IGNORE),
                    (DeathHint::OwnBody, label("click: watch a teammate", 14.0, DIM), hud_shadow(), Pickable::IGNORE),
                ],
            ),
            // Bottom right: when we come back.
            (
                Node {
                    position_type: PositionType::Absolute,
                    right: px(24),
                    bottom: px(22),
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (
                        Node {
                            width: px(76),
                            height: px(76),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(5)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        BorderColor::all(ACCENT),
                        Pickable::IGNORE,
                        children![(heading("", 30.0, TEXT), Hud::RespawnIn, Pickable::IGNORE)],
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3),
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            (weighted("RESPAWN IN", 12.0, 700, TEXT), hud_shadow(), Pickable::IGNORE),
                            (label("", 13.0, TEXT), Hud::RespawnHint, hud_shadow(), Pickable::IGNORE),
                        ],
                    ),
                ],
            ),
            // The loadout, over everything while it is open.
            (
                LoadoutOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: px(14),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                children![
                    death_loadout(),
                    (
                        Node {
                            width: px(760),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            column_gap: px(20),
                            ..default()
                        },
                        children![
                            (
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(12),
                                    padding: UiRect::new(px(6), px(16), px(8), px(8)),
                                    border_radius: BorderRadius::all(px(12)),
                                    flex_shrink: 1.0,
                                    ..default()
                                },
                                BackgroundColor(PANEL.with_alpha(0.85)),
                                children![
                                    (
                                        Node {
                                            padding: UiRect::axes(px(8), px(3)),
                                            border_radius: BorderRadius::all(px(8)),
                                            ..default()
                                        },
                                        BackgroundColor(BUTTON_HOVER),
                                        children![weighted("TIP", 11.0, 700, TEXT)],
                                    ),
                                    (label("", 14.0, TEXT), Hud::Tip),
                                ],
                            ),
                            (
                                Node {
                                    flex_shrink: 0.0,
                                    align_items: AlignItems::Center,
                                    column_gap: px(12),
                                    ..default()
                                },
                                children![
                                    (label("L / Esc", 13.0, DIM), hud_shadow()),
                                    (
                                        button("Back to watching"),
                                        LoadoutClose,
                                        observe(|_: On<Activate>, mut open: ResMut<DeathLoadoutOpen>| open.0 = false),
                                    ),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
        ],
    ));

    // The scoreboard, while the key is held: over everything else, which it
    // hides (see `update_overlays`).
    commands.spawn((
        hud_layer(),
        ScoreboardPanel,
        GlobalZIndex(12),
        children![(
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
                                (weighted("", 12.0, 700, DIM), Hud::ScoreCaption),
                                (heading("", 30.0, TEXT), Hud::ScoreMode),
                            ],
                        ),
                        (heading("", 30.0, ACCENT), Hud::ScoreClock),
                    ],
                ),
                roster_list(ROSTER_WIDTH, false),
                (
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        margin: UiRect::top(px(6)),
                        ..default()
                    },
                    children![label("Shows while you hold", 13.0, DIM), key_chip(Action::Scores)],
                ),
            ],
        )],
    ));

    // Pause menu: what to do, and how the match is going. It takes clicks.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        PausePanel,
        GlobalZIndex(10),
        BackgroundColor(SCRIM.with_alpha(0.66)),
        DespawnOnExit(AppState::Playing),
        children![(
            Node {
                // Room for five facts in a row.
                width: px(520),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(28)),
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
                        align_items: AlignItems::Baseline,
                        ..default()
                    },
                    children![heading("Paused", 34.0, TEXT), (weighted("", 14.0, 600, DIM), Hud::PauseClock)],
                ),
                (label("", 14.0, DIM), Hud::PauseInfo),
                (
                    Node {
                        column_gap: px(8),
                        margin: UiRect::vertical(px(8)),
                        ..default()
                    },
                    children![
                        pause_fact("Place", Hud::PausePlace),
                        (pause_fact("Kills", Hud::PauseKills), ArmedOnly),
                        (pause_fact("Assists", Hud::PauseAssists), TeamModeOnly),
                        (pause_fact("Deaths", Hud::PauseDeaths), ArmedOnly),
                        (pause_fact("Damage", Hud::PauseDamage), ArmedOnly),
                        (pause_fact("Time", Hud::PauseTime), CourseOnly),
                        (pause_fact("Checkpoints", Hud::PauseGates), CourseOnly),
                    ],
                ),
                (primary_button("Resume"), observe(on_resume)),
                ((button("Loadout"), observe(on_open_loadout)), ArmedOnly),
                (button("Settings"), observe(on_open_settings)),
                (
                    button("End match for everyone"),
                    LeaderOnly,
                    observe(send_on_click(ClientMsg::EndMatch)),
                ),
                (danger_button("Leave match"), observe(on_leave)),
            ],
        )],
    ));
}

/// One number on the pause menu, under its caption.
pub(super) fn pause_fact(caption: &str, hud: Hud) -> impl Bundle + use<> {
    (
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::axes(px(12), px(10)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(CARD),
        children![
            weighted(caption.to_uppercase(), 10.0, 700, DIM),
            // "2nd of 12" on one line: its card takes the room it needs.
            (weighted("", 18.0, 700, TEXT), TextLayout::no_wrap(), hud),
        ],
    )
}

/// One of these shows under the loadout on the death screen, a different one
/// each death.
pub(super) const TIPS: &[&str] = &[
    "Strafe and turn in the air to build speed.",
    "Green pads launch you: arrows throw you their way, plain ones keep your momentum.",
    "Bullets take time to arrive: lead moving targets.",
    "Team modes: your pings show only to your team.",
    "Health comes back after a few seconds without getting hit.",
    "Armor takes damage before health and never comes back by itself: grab the blue plates.",
    "Crouch while running to slide.",
];

/// The name of the key an action is bound to, in a little box.
#[derive(Component)]
pub struct KeyHint(pub Action);

pub(super) fn key_chip(action: Action) -> impl Bundle {
    (
        Node {
            min_width: px(28),
            padding: UiRect::axes(px(7), px(2)),
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(BUTTON),
        BorderColor::all(BUTTON_EDGE),
        children![(label("", 14.0, TEXT), KeyHint(action))],
    )
}

/// Keeps the keys in the pause menu in step with the bindings.
pub(super) fn update_key_hints(
    settings: Res<Settings>,
    added: Query<(), Added<KeyHint>>,
    mut hints: Query<(&KeyHint, &mut Text)>,
) {
    if !settings.is_changed() && added.is_empty() {
        return;
    }
    for (hint, mut text) in &mut hints {
        set_text(&mut text, settings.key_names(hint.0));
    }
}

/// A HUD element that says nothing while we are dead (health, speed, ammo),
/// hidden on the death screen.
#[derive(Component)]
pub struct HiddenWhenDead;

/// The death screen (see `spawn_hud`).
#[derive(Component)]
pub(super) struct DeathScreen;

/// The death screen's line of what we respawn with, which opens the loadout.
#[derive(Component)]
pub(super) struct DeathLoadoutSummary;

/// The teammate we see through, at the bottom of the death screen...
#[derive(Component)]
pub(super) struct WatchPanel;

/// ...or a word in its place.
#[derive(Component)]
pub(super) enum DeathHint {
    /// With none of them alive, that there is nobody to watch.
    Alone,
    /// Looking down on our own body, how to go and watch them.
    OwnBody,
}

/// The loadout over the death screen, and its button back to watching.
#[derive(Component)]
pub(super) struct LoadoutOverlay;

#[derive(Component)]
pub(super) struct LoadoutClose;

/// A click that reaches the death screen (it bubbles up from whatever was
/// under the pointer): on the view, the left button steps to the next
/// teammate and the right one back; with the loadout open, off its panel,
/// it shuts it. Clicks on the loadout, the map or their buttons are theirs.
pub(super) fn on_death_screen_click(
    click: On<PointerClick>,
    parents: Query<&ChildOf>,
    keep: Query<
        (),
        Or<(
            With<DeathLoadout>,
            With<DeathLoadoutSummary>,
            With<LoadoutClose>,
            With<crate::minimap::DeployMap>,
        )>,
    >,
    mut open: ResMut<DeathLoadoutOpen>,
    mut spectate: ResMut<Spectate>,
) {
    let target = click.original_event_target();
    if std::iter::once(target).chain(parents.iter_ancestors(target)).any(|entity| keep.contains(entity)) {
        return;
    }
    if open.0 {
        open.0 = false;
        return;
    }
    match click.button {
        PointerButton::Primary => spectate.step += 1,
        PointerButton::Secondary => spectate.step -= 1,
        _ => {}
    }
}

/// Shows the parts of the death screen that apply: the teammate, the word
/// that there is none or how to watch one, and the loadout panel while it is open.
pub(super) fn update_death_screen(
    spectate: Res<Spectate>,
    open: Res<DeathLoadoutOpen>,
    mut nodes: ParamSet<(
        Query<&mut Node, With<WatchPanel>>,
        Query<(&mut Node, &DeathHint)>,
        Query<&mut Node, With<LoadoutOverlay>>,
    )>,
) {
    for mut node in &mut nodes.p0() {
        set_shown(&mut node, spectate.watching.is_some());
    }
    for (mut node, hint) in &mut nodes.p1() {
        let shown = match hint {
            DeathHint::Alone => spectate.alone,
            DeathHint::OwnBody => spectate.can_watch,
        };
        set_shown(&mut node, shown);
    }
    for mut node in &mut nodes.p2() {
        set_shown(&mut node, open.0);
    }
}

pub(super) fn on_resume(_: On<Activate>, mut controls: ResMut<Controls>) {
    controls.captured = true;
}

/// The match's clock as shown: the time left, or for a time trial, which
/// has no time limit, none.
pub(super) fn match_clock(session: &Session) -> String {
    if modes::info(session.mode).timed { format_clock(session.time_left) } else { "∞".to_string() }
}

pub(super) fn format_clock(seconds: f32) -> String {
    let total = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The frame rate on the HUD: frames counted over a quarter second, so the
/// number holds still long enough to read.
#[derive(Default)]
pub(super) struct FpsCounter {
    pub(super) frames: u32,
    pub(super) elapsed: f32,
    pub(super) shown: f32,
}

impl FpsCounter {
    pub(super) fn tick(&mut self, delta: f32) {
        self.frames += 1;
        self.elapsed += delta;
        if self.elapsed >= 0.25 {
            self.shown = self.frames as f32 / self.elapsed;
            self.frames = 0;
            self.elapsed = 0.0;
        }
    }
}

pub(super) fn update_hud(
    session: Res<Session>,
    map: Res<GameMap>,
    player: Res<LocalPlayer>,
    feed: Res<KillFeed>,
    notices: Res<Notices>,
    connection: Option<Res<Connection>>,
    time: Res<Time>,
    mut fps: Local<FpsCounter>,
    spectate: Res<Spectate>,
    settings: Res<Settings>,
    mut texts: Query<(&Hud, &mut Text, &mut TextColor)>,
) {
    fps.tick(time.delta_secs());
    let ping = connection.as_ref().map_or(0.0, |c| c.ping_ms());
    let mode_hud = &modes::info(session.mode).hud;
    let top = (mode_hud.top_bar)(&session);
    let my_team = session.team_of(session.my_id);
    let map_name = crate::map::name(map.0.id);
    let mode_name = modes::name(session.mode);
    let me = session.roster.iter().find(|p| p.id == session.my_id);
    // Where we stand: best score first, as on the scoreboard.
    let place = {
        let mut order: Vec<_> = session.roster.iter().collect();
        order.sort_by(|a, b| (mode_hud.rank)(a, b));
        let at = order.iter().position(|p| p.id == session.my_id).map_or(0, |i| i + 1);
        format!("{} of {}", ordinal(at), order.len())
    };
    let gates = map.0.course.as_ref().map_or(0, |course| course.gates.len());
    let restart = settings.key_name(Action::Reload).to_uppercase();
    // Who killed us, without the bot tag, and how many kills they have.
    let killer_name = player.killed_by.as_deref().unwrap_or("?");
    let killer = killer_name.strip_prefix("[bot] ").unwrap_or(killer_name).to_string();
    let killer_info = session.roster.iter().find(|p| p.name == killer_name);
    let by_myself = killer_info.is_some_and(|p| p.id == session.my_id);
    let killer_kills = killer_info.map(|p| p.kills).filter(|_| !by_myself);
    let killer_leads = killer_info.is_some_and(|k| session.roster.iter().all(|p| p.id == k.id || p.kills < k.kills));
    let killed_with = match player.killed_with {
        Some(crate::weapons::GRENADE) => "With a grenade".to_string(),
        Some(id) if (id as usize) < crate::weapons::WEAPONS.len() => format!("With their {}", crate::weapons::weapon(id).name),
        _ => String::new(),
    };
    // A notice fades out over its last half second.
    let notice = |notice: &Option<(String, f32)>, color: Color| match notice {
        Some((text, left)) => (text.clone(), color.with_alpha((left / 0.5).min(1.0))),
        None => (String::new(), color),
    };
    for (hud, mut text, mut color) in &mut texts {
        let (value, wanted) = match *hud {
            Hud::Clock => (
                match_clock(&session),
                // The last half minute.
                if session.time_left <= 30.0 { DANGER } else { TEXT },
            ),
            Hud::ScoreLabel(side) => (top.labels[side].clone(), top.colors[side].with_alpha(0.8)),
            Hud::Score(side) => (top.scores[side].clone(), top.colors[side]),
            Hud::Info => (top.info.clone(), DIM),
            Hud::Fps => (
                format!("{:.0}", fps.shown),
                match fps.shown {
                    f if f < 30.0 => DANGER,
                    f if f < 60.0 => ACCENT,
                    _ => TEXT,
                },
            ),
            Hud::Ping => (
                format!("{ping:.0} ms"),
                match ping {
                    p if p > 150.0 => DANGER,
                    p if p > 80.0 => ACCENT,
                    _ => TEXT,
                },
            ),
            Hud::Speed => (format!("{:.1} m/s", player.horizontal_speed()), TEXT),
            Hud::DashLabel => (
                "DASH".to_string(),
                if player.dash_cooldown <= 0.0 { TEXT } else { DIM },
            ),
            Hud::Feed(row, part) => match feed.0.get(row) {
                Some(line) => {
                    let fade = (line.left / 0.5).min(1.0);
                    let named = match part {
                        FeedPart::Killer => line.killer.as_ref(),
                        FeedPart::Victim => line.victim.as_ref(),
                        FeedPart::Tail => None,
                    };
                    match (part, named) {
                        (FeedPart::Tail, _) => (line.tail.clone(), TEXT.with_alpha(fade)),
                        (_, Some((name, team))) => {
                            let color = team.map_or(TEXT, |team| team_color(my_team, team));
                            (name.clone(), color.with_alpha(fade))
                        }
                        _ => (String::new(), TEXT),
                    }
                }
                None => (String::new(), TEXT),
            },
            Hud::Center => {
                let ready = !player.alive && player.killed_by.is_none();
                (if ready { "Get ready..." } else { "" }.to_string(), DANGER)
            }
            Hud::Killer => (if by_myself { "YOURSELF".to_string() } else { killer.to_uppercase() }, RED),
            Hud::KillerNote => {
                let tally = match killer_kills {
                    Some(kills) if killer_leads => format!("that's {kills} for {killer}, who leads"),
                    Some(1) => format!("the first for {killer}"),
                    Some(kills) => format!("that's {kills} for {killer}"),
                    None => String::new(),
                };
                let note = match (killed_with.is_empty(), tally.is_empty()) {
                    (false, false) => format!("{killed_with}  ·  {tally}"),
                    (false, true) => killed_with.clone(),
                    (true, false) => tally,
                    (true, true) => String::new(),
                };
                (note, TEXT)
            }
            Hud::Watching => (
                match spectate.watching {
                    Some(id) => {
                        let name = session.name_of(id);
                        format!("‹   {}   ›", name.strip_prefix("[bot] ").unwrap_or(name).to_uppercase())
                    }
                    None => String::new(),
                },
                BLUE,
            ),
            Hud::WatchedHealth => (spectate.health.max(0).to_string(), TEXT),
            Hud::WatchedArmor => (if spectate.armor > 0 { spectate.armor.to_string() } else { String::new() }, ARMOR),
            Hud::WatchedGun => {
                let gun = crate::weapons::weapon(spectate.weapon);
                let on = crate::weapons::sight(spectate.sight);
                let line = if spectate.sight == crate::weapons::IRON_SIGHTS || spectate.weapon == crate::weapons::KNIFE {
                    gun.name.to_string()
                } else {
                    format!("{}  ·  {}", gun.name, on.name)
                };
                (line, TEXT)
            }
            Hud::RespawnIn => {
                let left = (RESPAWN_DELAY - player.dead_for).max(0.0).ceil();
                (if player.respawn_asked { "…".to_string() } else { format!("{left:.0}") }, TEXT)
            }
            Hud::RespawnHint => (
                if modes::info(session.mode).early_respawn {
                    format!("{}: now", settings.key_name(Action::Jump).to_uppercase())
                } else {
                    String::new()
                },
                TEXT,
            ),
            Hud::Tip => (TIPS[me.map_or(0, |p| p.deaths as usize) % TIPS.len()].to_string(), TEXT),
            Hud::PauseClock if !modes::info(session.mode).timed => ("No time limit".to_string(), DIM),
            Hud::PauseClock => (format!("{} left", format_clock(session.time_left)), DIM),
            Hud::PauseInfo => (format!("{map_name}  ·  {mode_name}  ·  the match keeps going"), DIM),
            Hud::PausePlace => (place.clone(), TEXT),
            Hud::PauseKills => (me.map_or(0, |p| p.kills).to_string(), TEXT),
            Hud::PauseAssists => (me.map_or(0, |p| p.assists).to_string(), TEXT),
            Hud::PauseDeaths => (me.map_or(0, |p| p.deaths).to_string(), TEXT),
            Hud::PauseDamage => (me.map_or(0, |p| p.damage).to_string(), TEXT),
            Hud::ScoreCaption => {
                let wins = (mode_hud.wins)(&session);
                (format!("{}  ·  {wins}", map_name.to_uppercase()), DIM)
            }
            Hud::ScoreMode => (mode_name.to_uppercase(), TEXT),
            Hud::ScoreClock => (match_clock(&session), ACCENT),
            Hud::Hand => match player.hand.filter(|_| player.alive) {
                Some(hand) => {
                    let name = GRENADES[hand.kind as usize].name.to_uppercase();
                    match hand.fuse {
                        // About to go off in hand: red.
                        Some(fuse) => (format!("{name}  {:.1}", fuse.max(0.0)), if fuse < 0.8 { DANGER } else { ACCENT }),
                        None if hand.kind == FRAG => (format!("{name}  -  click to pull the pin"), TEXT),
                        None => (format!("{name}  -  click to throw"), TEXT),
                    }
                }
                None => (String::new(), ACCENT),
            },
            Hud::Kill => notice(&notices.kill, TEXT),
            Hud::Banner => notice(&notices.banner, ACCENT),
            Hud::Countdown => match session.countdown {
                left if left > 0.0 => (format!("{}", left.ceil() as u32), ACCENT),
                // "GO" for a moment once it is over, fading out.
                left if left > -0.8 => ("GO!".to_string(), GOOD.with_alpha(((left + 0.8) / 0.4).min(1.0))),
                _ => (String::new(), ACCENT),
            },
            Hud::CountdownLabel => (
                if session.frozen() { mode_hud.countdown } else { "" }.to_string(),
                TEXT,
            ),
            Hud::RunClock => match (player.racing, player.run.finished, player.run.clock) {
                (Racing::No, ..) => (String::new(), TEXT),
                (_, Some(time), _) => (format_time(time), GOOD),
                (_, None, Some(time)) => (format_time(time), TEXT),
                (_, None, None) => (format_time(0.0), DIM),
            },
            Hud::RunInfo => {
                let line = match (player.racing, player.run.finished, player.run.clock) {
                    (Racing::No, ..) => String::new(),
                    _ if !player.alive || session.frozen() => String::new(),
                    (Racing::Trial, Some(_), _) => {
                        format!("back to the start in {:.0}", (BACK_TO_START - player.finished_for).max(0.0).ceil())
                    }
                    (Racing::Race, Some(_), _) => "through the finish".to_string(),
                    (Racing::Trial, None, None) => format!("leave the start to begin   ·   {restart}  back to the start"),
                    (Racing::Trial, None, Some(_)) => {
                        format!("checkpoint {}/{gates}   ·   {restart}  back to the start", player.run.passed)
                    }
                    (Racing::Race, None, _) => {
                        format!("checkpoint {}/{gates}   ·   {restart}  back to the last one", player.run.passed)
                    }
                };
                (line, DIM)
            }
            Hud::PauseTime => (me.and_then(|p| p.time()).map_or("-".to_string(), format_time), TEXT),
            Hud::PauseGates => (format!("{}/{gates}", player.run.passed), TEXT),
        };
        set_text(&mut text, value);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// The bars, and the boxes that light up or come and go.
pub(super) fn update_hud_shapes(
    time: Res<Time>,
    player: Res<LocalPlayer>,
    spectate: Res<Spectate>,
    feed: Res<KillFeed>,
    mut health_lag: Local<f32>,
    mut bars: Query<(&Bar, &mut Node, &mut BackgroundColor)>,
    mut rows: Query<(&FeedRow, &mut Node, &mut BackgroundColor), Without<Bar>>,
    mut pictures: Query<(&FeedWeapon, &mut Node, &mut ImageNode), (Without<Bar>, Without<FeedRow>)>,
    thumbnails: Res<Thumbnails>,
) {
    let health = if player.alive { player.health.max(0) } else { 0 };
    let health_part = health as f32 / MAX_HEALTH as f32;
    // What was just lost drains away behind the bar; healing catches up at once.
    *health_lag = (*health_lag - time.delta_secs() * 0.6).max(health_part);
    let health_color = if health <= LOW_HEALTH {
        // Pulses, to be hard to miss.
        DANGER.with_alpha(0.75 + 0.25 * (time.elapsed_secs() * 9.0).sin())
    } else if health <= MAX_HEALTH * 6 / 10 {
        ACCENT
    } else {
        GOOD
    };

    let dash_part = 1.0 - player.dash_cooldown / player.moves.dash_cooldown.max(0.01);
    let armor_part = if player.alive { player.armor.max(0) as f32 / MAX_ARMOR as f32 } else { 0.0 };

    for (bar, mut node, mut background) in &mut bars {
        let (part, color) = match *bar {
            Bar::Health => (health_part, health_color),
            Bar::HealthLag => (*health_lag, Color::srgba(1.0, 1.0, 1.0, 0.75)),
            Bar::Armor => (armor_part, ARMOR),
            Bar::Dash => (dash_part, if dash_part >= 1.0 { ACCENT } else { DIM }),
            Bar::WatchedHealth => {
                let part = spectate.health.max(0) as f32 / MAX_HEALTH as f32;
                (part, if spectate.health <= LOW_HEALTH { DANGER } else { GOOD })
            }
            Bar::WatchedArmor => (spectate.armor.max(0) as f32 / MAX_ARMOR as f32, ARMOR),
        };
        let width = percent(part.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
        if background.0 != color {
            background.0 = color;
        }
    }

    for (row, mut node, mut background) in &mut rows {
        let left = feed.0.get(row.0).map(|line| line.left);
        set_shown(&mut node, left.is_some());
        // Fades out with its text.
        let color = HUD_PANEL.with_alpha(HUD_PANEL.alpha() * (left.unwrap_or(0.0) / 0.5).min(1.0));
        if background.0 != color {
            background.0 = color;
        }
    }

    for (picture, mut node, mut image) in &mut pictures {
        let line = feed.0.get(picture.0);
        let preview = line.and_then(|line| line.weapon).map(|weapon_id| match weapon_id {
            GRENADE => Preview::Grenade(FRAG),
            id => Preview::Gun(id, IRON_SIGHTS),
        });
        let handle = preview.and_then(|preview| thumbnails.get(preview));
        set_shown(&mut node, handle.is_some());
        if let Some(handle) = handle
            && image.image != handle
        {
            image.image = handle;
        }
        let color = Color::WHITE.with_alpha(line.map_or(0.0, |line| (line.left / 0.5).min(1.0)));
        if image.color != color {
            image.color = color;
        }
    }
}

/// The view through a scope (see `LocalPlayer::scope_view`): the lens in the
/// middle, and a black bar on each side of it.
#[derive(Component)]
pub(super) struct ScopeView;
#[derive(Component)]
pub(super) struct ScopeLens;
#[derive(Component)]
pub(super) struct ScopeSide;

/// Pixels across the scope's lens texture.
pub(super) const SCOPE_LENS_SIZE: u32 = 1024;

/// A square texture of one colour, `size` pixels across, see-through by
/// `alpha(u, v, pixel)`: `u` and `v` run from -1 to 1 across and down, and
/// `pixel` is the width of one pixel in those units.
pub fn mask_image(size: u32, rgb: [u8; 3], alpha: impl Fn(f32, f32, f32) -> f32) -> Image {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    let pixel = 2.0 / size as f32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) * pixel - 1.0;
            let v = (y as f32 + 0.5) * pixel - 1.0;
            let a = alpha(u, v, pixel).clamp(0.0, 1.0);
            data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// A scope's lens: clear in the middle, with a duplex reticle (thin lines
/// through the centre, thick posts towards the edge), darkening to black at
/// the rim and beyond it.
pub(super) fn scope_lens_image() -> Image {
    let rim = 0.97;
    let (thin, thick, posts_from) = (0.0026, 0.012, 0.36);
    mask_image(SCOPE_LENS_SIZE, [0, 0, 0], |u, v, pixel| {
        // How much of a pixel lies within `half` of a line `distance` away.
        let cover = |distance: f32, half: f32| ((half - distance) / pixel + 0.5).clamp(0.0, 1.0);
        let r = (u * u + v * v).sqrt();
        let outside = ((r - rim) / pixel + 0.5).clamp(0.0, 1.0);
        let shade = ((r - (rim - 0.16)) / 0.16).clamp(0.0, 1.0).powi(2) * 0.85;
        let posts =
            |along: f32, across: f32| cover(across.abs(), thick) * ((along.abs() - posts_from) / pixel + 0.5).clamp(0.0, 1.0);
        let reticle = cover(u.abs(), thin)
            .max(cover(v.abs(), thin))
            .max(posts(u, v))
            .max(posts(v, u));
        outside.max(shade).max(reticle)
    })
}

/// How many hits from different sides can show at once.
pub(super) const DAMAGE_ARCS: usize = 6;
/// Pixels across the ring the arcs lie on.
pub(super) const DAMAGE_RING: f32 = 170.0;

/// One of the arcs around the crosshair that point to where a hit came from
/// (see `LocalPlayer::damage_arcs`).
#[derive(Component)]
pub(super) struct DamageArcNode(pub(super) usize);

/// A thin arc at the top of a ring, fading towards its ends. Turned about the
/// middle, it points any way.
pub(super) fn damage_arc_image() -> Image {
    let (radius, half_width, half_angle) = (0.9, 0.035, 0.38);
    mask_image(256, [255, 255, 255], |u, v, pixel| {
        let r = (u * u + v * v).sqrt();
        // Clockwise from straight up.
        let angle = u.atan2(-v);
        let across = ((half_width - (r - radius).abs()) / pixel + 0.5).clamp(0.0, 1.0);
        let along = (1.0 - angle.abs() / half_angle).clamp(0.0, 1.0);
        across * along.powf(0.6)
    })
}

pub(super) fn update_damage_arcs(
    player: Res<LocalPlayer>,
    mut arcs: Query<(&DamageArcNode, &mut ImageNode, &mut UiTransform)>,
) {
    for (arc, mut image, mut transform) in &mut arcs {
        let Some(hit) = player.damage_arcs.get(arc.0) else {
            if image.color != Color::NONE {
                image.color = Color::NONE;
            }
            continue;
        };
        // Full for a moment, then it fades.
        let fade = (hit.left / DAMAGE_ARC_TIME * 1.6).min(1.0);
        image.color = Color::srgba(1.0, 0.3, 0.22, 0.85 * fade);
        transform.rotation = Rot2::radians(player.bearing(hit.at));
    }
}

pub(super) fn update_scope_view(
    player: Res<LocalPlayer>,
    mut view: Single<&mut Visibility, With<ScopeView>>,
    mut lens: Single<&mut ImageNode, With<ScopeLens>>,
    mut sides: Query<&mut BackgroundColor, With<ScopeSide>>,
) {
    let fade = player.scope_view();
    view.set_if_neq(if fade > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    let color = Color::srgba(1.0, 1.0, 1.0, fade);
    if lens.color != color {
        lens.color = color;
    }
    for mut side in &mut sides {
        side.set_if_neq(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, fade)));
    }
}

/// The crosshair's dot, hidden while aiming through a sight that has its own.
#[derive(Component)]
pub(super) struct Crosshair;

pub(super) fn update_crosshair(
    player: Res<LocalPlayer>,
    fitted: Res<FittedModels>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
) {
    // Aimed down a sight, or iron sights that line up, the crosshair would only get in the way.
    let sight = fitted
        .get(player.weapon_id())
        .is_some_and(|f| f.aim_point(player.sight_id()).or(f.iron_aim).is_some());
    crosshair.set_if_neq(if sight && player.aim > 0.9 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
}

/// Shows how wide a choked gun's shots will spread (see `Weapon::choke`).
#[derive(Component)]
pub(super) struct ChokeRing;

pub(super) fn update_choke_ring(
    player: Res<LocalPlayer>,
    window: Single<&Window, With<PrimaryWindow>>,
    projection: Single<&Projection, With<MainCamera>>,
    ring: Single<(&mut Node, &mut BorderColor, &mut Visibility), With<ChokeRing>>,
    ui_scale: Res<UiScale>,
) {
    let (mut node, mut border, mut visibility) = ring.into_inner();
    let shown = player.alive && player.weapon().choke.is_some() && player.aim > 0.05;
    visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    if !shown {
        return;
    }
    let fov = match *projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => 1.0,
    };
    // Where the edge of the cone lands on the screen.
    let radius = player.spread().tan() / (fov / 2.0).tan() * ui_size(&window, &ui_scale).y / 2.0;
    let size = px((radius * 2.0).round());
    if node.width != size {
        node.width = size;
        node.height = size;
    }
    let color = if player.choke() >= 1.0 {
        ACCENT.with_alpha(0.85 * player.aim)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.55 * player.aim)
    };
    if border.top != color {
        *border = BorderColor::all(color);
    }
}

pub(super) fn update_overlays(
    input: ActionInput,
    session: Res<Session>,
    player: Res<LocalPlayer>,
    smoke_fog: Res<SmokeFog>,
    controls: Res<Controls>,
    mut overlays: Query<(&Overlay, &mut BackgroundColor)>,
    mut nodes: ParamSet<(
        Single<&mut Node, With<ScoreboardPanel>>,
        Single<&mut Node, With<PausePanel>>,
        Query<&mut Node, With<LeaderOnly>>,
        Single<&mut Node, With<DeathScreen>>,
        Query<(&mut Node, Has<ArmedOnly>), With<HiddenWhenDead>>,
        Query<&mut Node, With<TeamModeOnly>>,
        Query<&mut Node, (With<ArmedOnly>, Without<HiddenWhenDead>)>,
        Query<&mut Node, With<CourseOnly>>,
    )>,
) {
    // White ticks for a hit, yellow for a headshot; red, and held a little
    // longer, for a kill.
    let tick_color = if player.kill_marker > 0.0 {
        Color::srgba(1.0, 0.15, 0.1, (player.kill_marker * 3.0).min(1.0))
    } else if player.headshot_marker > 0.0 {
        Color::srgba(1.0, 0.85, 0.1, (player.headshot_marker / HIT_MARKER_TIME * 2.0).clamp(0.0, 1.0))
    } else {
        Color::srgba(1.0, 1.0, 1.0, (player.hit_marker / HIT_MARKER_TIME * 2.0).clamp(0.0, 1.0))
    };
    for (overlay, mut background) in &mut overlays {
        let wanted = match overlay {
            Overlay::DamageFlash => Color::srgba(0.8, 0.0, 0.0, player.damage_flash * 0.35),
            // Pure white until the last second, then it clears.
            Overlay::Blind => Color::srgba(1.0, 1.0, 1.0, player.blind.min(1.0)),
            Overlay::Smoke => Color::srgba(0.6, 0.62, 0.62, smoke_fog.0 * 0.97),
            Overlay::HitTick => tick_color,
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }

    // One of the scoreboard, the pause menu and the death screen at a time,
    // in that order.
    let show_scores = input.pressed(Action::Scores);
    let paused = !controls.captured && !show_scores;
    let dead = player.on_death_screen() && !paused && !show_scores;
    set_shown(&mut nodes.p3(), dead);
    // What is on screen for a gun goes by what we have this life; the rest
    // by whether the mode has guns at all.
    let armed = modes::info(session.mode).armed;
    let in_hand = armed && !player.unarmed;
    for (mut node, armed_only) in &mut nodes.p4() {
        set_shown(&mut node, !player.on_death_screen() && (in_hand || !armed_only));
    }
    for mut node in &mut nodes.p6() {
        set_shown(&mut node, armed);
    }
    for mut node in &mut nodes.p7() {
        set_shown(&mut node, is_course_mode(session.mode));
    }
    set_shown(&mut nodes.p0(), show_scores);
    set_shown(&mut nodes.p1(), paused);
    for mut node in &mut nodes.p2() {
        set_shown(&mut node, session.is_leader());
    }
    for mut node in &mut nodes.p5() {
        set_shown(&mut node, session.team_mode());
    }
}

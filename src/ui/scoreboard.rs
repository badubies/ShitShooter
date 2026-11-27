//! The list of players and their scores: the scoreboard in a match, and the
//! last match's in the lobby.

use super::*;

/// A table of the players and their scores, kept up to date by
/// `update_rosters`. Used for the in-game scoreboard, and in the lobby for
/// how the last match ended.
#[derive(Component, Default)]
pub(super) struct RosterList {
    /// What the rows were last built from.
    pub(super) shown: Option<RosterKey>,
    /// Shows the scores the last match ended on (`Session::last_match`), not
    /// the players here now.
    pub(super) last_match: bool,
}

/// Everything the player list shows; the rows are rebuilt when it changes.
#[derive(Clone, PartialEq)]
pub(super) struct RosterKey {
    pub(super) roster: Vec<PlayerInfo>,
    pub(super) my_id: PlayerId,
    pub(super) leader: PlayerId,
    /// The mode it was played in: that decides the columns, and whether
    /// the players are listed by team.
    pub(super) mode: u8,
    pub(super) my_team: u8,
    pub(super) team_scores: [u32; 2],
    /// The dot beside each name, in roster order: their main paint colour.
    pub(super) colors: Vec<u8>,
}

/// How wide one column of the scoreboard is: a team, or half of free for all.
pub(super) const ROSTER_COLUMN_WIDTH: f32 = 490.0;
/// How wide the scoreboard's player list is at least.
pub(super) const ROSTER_WIDTH: f32 = ROSTER_COLUMN_WIDTH;
pub(super) const STAT_WIDTH: f32 = 52.0;
/// Longer names are cut off at this width.
pub(super) const ROSTER_NAME_WIDTH: f32 = 150.0;

pub(super) fn roster_list(min_width: f32, last_match: bool) -> impl Bundle {
    (
        RosterList {
            shown: None,
            last_match,
        },
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            min_width: px(min_width),
            ..default()
        },
    )
}

/// Text that keeps to the outside edge of its column: the right edge on the
/// right-hand team, which runs mirrored, and the left one elsewhere.
pub(super) fn outer_justify(right: bool) -> TextLayout {
    TextLayout::justify(if right { Justify::Right } else { Justify::Left })
}

/// What goes in one of the columns beside the names (`wide`: in a column
/// twice the width, for a time). They keep to the names' side, so on the
/// mirrored team they line up on their left.
pub(super) fn roster_cell(text: String, size: f32, color: Color, right: bool, wide: bool) -> impl Bundle {
    (
        label(text, size, color),
        TextLayout::justify(if right { Justify::Left } else { Justify::Right }),
        Node {
            width: px(if wide { STAT_WIDTH * 2.0 } else { STAT_WIDTH }),
            flex_shrink: 0.0,
            ..default()
        },
    )
}

pub(super) fn roster_row_node(right: bool) -> Node {
    Node {
        align_items: AlignItems::Center,
        flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row },
        column_gap: px(8),
        padding: UiRect::axes(px(10), px(6)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(10)),
        ..default()
    }
}

/// The line over a group of players: a title, and the names of the mode's
/// columns (`ModeHud::columns`).
pub(super) fn roster_heading(title: String, color: Color, right: bool, columns: &'static [Column]) -> impl Bundle {
    (
        roster_row_node(right),
        Children::spawn((
            Spawn((weighted(title, 11.0, 700, color), outer_justify(right), Node { flex_grow: 1.0, ..default() })),
            SpawnIter(columns.iter().map(move |column| roster_cell(column.title.to_string(), 11.0, DIM, right, column.wide))),
        )),
    )
}

/// Over a team's players: its name and score, big, in its colour.
pub(super) fn roster_team_banner(name: &str, score: u32, color: Color, right: bool) -> impl Bundle + use<> {
    (
        Node {
            align_items: AlignItems::Baseline,
            justify_content: JustifyContent::SpaceBetween,
            flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row },
            padding: UiRect::axes(px(10), px(0)),
            margin: UiRect::bottom(px(2)),
            ..default()
        },
        children![
            (heading(name, 26.0, color), TextLayout::no_wrap()),
            (heading(&score.to_string(), 26.0, color), TextLayout::no_wrap()),
        ],
    )
}

pub(super) fn roster_row(player: &PlayerInfo, swatch: Color, key: &RosterKey, right: bool) -> impl Bundle + use<> {
    let me = player.id == key.my_id;
    let cells: Vec<(String, Color, bool)> = modes::info(key.mode)
        .hud
        .columns
        .iter()
        .map(|column| ((column.value)(player), if column.bright { TEXT } else { DIM }, column.wide))
        .collect();
    let (name, tag) = match player.name.strip_prefix("[bot] ") {
        Some(name) if player.is_bot => (name, "BOT"),
        _ if player.id == key.leader => (player.name.as_str(), "LEADER"),
        _ => (player.name.as_str(), ""),
    };
    (
        roster_row_node(right),
        // Your own line stands out.
        BackgroundColor(if me { ACCENT.with_alpha(0.10) } else { CARD }),
        BorderColor::all(if me { ACCENT.with_alpha(0.5) } else { Color::NONE }),
        Children::spawn((
            Spawn((
                Node {
                    width: px(14),
                    height: px(14),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(swatch),
            )),
            // Long names are cut short so both teams' columns stay put.
            Spawn((
                Node {
                    max_width: px(ROSTER_NAME_WIDTH),
                    overflow: Overflow::clip(),
                    flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row },
                    ..default()
                },
                children![(weighted(name.to_string(), 16.0, 600, TEXT), TextLayout::no_wrap(), Node {
                    flex_shrink: 0.0,
                    ..default()
                })],
            )),
            Spawn((
                weighted(tag, 10.0, 700, if tag == "LEADER" { ACCENT } else { DIM }),
                outer_justify(right),
                Node { flex_grow: 1.0, ..default() },
            )),
            SpawnIter(cells.into_iter().map(move |(text, color, wide)| roster_cell(text, 16.0, color, right, wide))),
        )),
    )
}

/// Rebuilds the rows of every player list whose players or scores changed.
/// Best first, as the mode ranks them (`ModeHud::rank`); one group per team
/// in the team modes.
pub(super) fn update_rosters(
    mut commands: Commands,
    session: Res<Session>,
    skins: Res<Skins>,
    my_paint: Option<Res<crate::paint::MyPaint>>,
    mut lists: Query<(Entity, &mut RosterList)>,
) {
    if lists.is_empty() {
        return;
    }
    let key_of = |roster: &[PlayerInfo], mode: u8, my_team: u8, team_scores: [u32; 2]| {
        let mut roster = roster.to_vec();
        roster.sort_by(modes::info(mode).hud.rank);
        let colors = roster
            .iter()
            .map(|player| match &my_paint {
                Some(mine) if player.id == session.my_id => mine.canvas.main_color(),
                _ => skins.main_color(player.id).unwrap_or(player.color),
            })
            .collect();
        RosterKey {
            roster,
            my_id: session.my_id,
            leader: session.leader,
            mode,
            my_team,
            team_scores,
            colors,
        }
    };
    let live = key_of(&session.roster, session.mode, session.team_of(session.my_id), session.team_scores);
    let last = session
        .last_match
        .as_ref()
        .map(|last| key_of(&last.roster, last.mode, last.my_team, last.team_scores));
    for (entity, mut list) in &mut lists {
        let key = match (list.last_match, &last) {
            (false, _) => &live,
            (true, Some(last)) => last,
            (true, None) => continue,
        };
        if list.shown.as_ref() == Some(key) {
            continue;
        }
        list.shown = Some(key.clone());
        let columns = modes::info(key.mode).hud.columns;
        let column = || Node {
            width: px(ROSTER_COLUMN_WIDTH),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            ..default()
        };
        commands.entity(entity).despawn_children().with_children(|groups| {
            // Side by side, like the lobby, so a full match fits the screen.
            groups.spawn(Node::default()).with_children(|sides| {
                if !is_team_mode(key.mode) {
                    // The best eight on the left, any more on the right.
                    let everyone: Vec<_> = key.roster.iter().zip(&key.colors).collect();
                    for (k, players) in everyone.chunks(TEAM_SIZE).enumerate() {
                        if k > 0 {
                            sides.spawn(Node {
                                width: px(32),
                                flex_shrink: 0.0,
                                ..default()
                            });
                        }
                        sides.spawn(column()).with_children(|rows| {
                            rows.spawn(roster_heading("PLAYER".into(), DIM, false, columns));
                            for (player, color) in players {
                                rows.spawn(roster_row(player, palette_color(**color), &key, false));
                            }
                        });
                    }
                    return;
                }
                // Our team (blue) on the left, the enemy (red) facing it on the
                // right, as in the lobby.
                let mine = key.my_team & 1;
                for team in [mine, mine ^ 1] {
                    let color = team_color(mine, team);
                    let right = team != mine;
                    if right {
                        sides.spawn(team_seam());
                    }
                    sides.spawn(column()).with_children(|rows| {
                        let name = format!("{} team", team_name(mine, team));
                        rows.spawn(roster_team_banner(&name, key.team_scores[team as usize], color, right));
                        rows.spawn(roster_heading("PLAYER".into(), DIM, right, columns));
                        for player in key.roster.iter().filter(|player| player.team & 1 == team) {
                            rows.spawn(roster_row(player, color, &key, right));
                        }
                    });
                }
            });
        });
    }
}

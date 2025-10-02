//! This example shows how to setup a simple counter using feathers widgets

use bevy::{
    feathers::{
        controls::{button, ButtonProps, ButtonVariant},
        dark_theme::create_dark_theme,
        theme::{ThemeBackgroundColor, ThemedText, UiTheme},
        tokens, FeathersPlugins,
    },
    prelude::*,
    ui_widgets::{observe, Activate},
};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, FeathersPlugins))
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, setup)
        .add_systems(Update, update_text)
        .run();
}

fn setup(mut commands: Commands) {
    // ui camera
    commands.spawn(Camera2d);
    commands.spawn(demo_root());
}

#[derive(Component)]
struct Counter {
    state: i32,
}

fn demo_root() -> impl Bundle {
    (
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        },
        ThemeBackgroundColor(tokens::WINDOW_BG),
        children![(
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Start,
                column_gap: px(8),
                ..default()
            },
            children![
                // (
                //     button(
                //         ButtonProps::default(),
                //         (),
                //         Spawn((Text::new("-"), ThemedText))
                //     ),
                //     observe(|_: On<Activate>, mut counter: Single<&mut Counter>| {
                //         counter.state -= 1;
                //     })
                // ),
                // (Text("0".into()), Counter { state: 0 }),
                // (
                //     button(
                //         ButtonProps {
                //             variant: ButtonVariant::Primary,
                //             ..default()
                //         },
                //         (),
                //         Spawn((Text::new("+"), ThemedText))
                //     ),
                //     observe(|_: On<Activate>, mut counter: Single<&mut Counter>| {
                //         counter.state += 1;
                //     })
                // ),
                labelled_slider("temp", 100.0, 50.0, ())
            ]
        )],
    )
}

fn labelled_slider(label: &str, max: f32, value: f32, b: impl Bundle) -> impl Bundle {
    (
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::Start,
            width: Val::Percent(100.0),
            ..Default::default()
        },
        children![
            Text(label.into()),
            (
                slider(
                    SliderProps {
                        max,
                        value,
                        ..Default::default()
                    },
                    (SliderStep(1.), SliderPrecision(1)),
                ),
                b
            )
        ],
    )
}

fn update_text(mut counter_changed: Query<(&Counter, &mut Text), Changed<Counter>>) {
    for (counter, mut text) in &mut counter_changed {
        text.0 = format!("{}", counter.state);
    }
}

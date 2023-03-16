use bevy_ecs::{
    prelude::Entity,
    schedule::{IntoSystemConfig, SystemConfig},
    system::{BoxedSystem, IntoSystem, System},
    world::World,
};

// TODO port main,

type RenderSystemIn = Option<Entity>;

struct RenderGraphV2<In = RenderSystemIn, Out = ()> {
    systems: Vec<Option<BoxedSystem<In, Out>>>,
}

impl RenderGraphV2 {
    pub fn add_node<M: Sized>(&mut self, system: impl IntoSystem<RenderSystemIn, (), M>) {
        let sys = Box::new(IntoSystem::into_system(system));
        self.systems.push(Some(sys));
    }

    pub fn init(&mut self, world: &mut World) {
        for system in &mut self.systems {
            let Some(system) = system else { continue; };
            system.initialize(world);
        }
    }

    pub fn run(&mut self, world: &mut World) {
        let view_entity = Entity::PLACEHOLDER;

        for system in &mut self.systems {
            let Some(system) = system else { continue; };
            system.run(Some(view_entity), world);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::{
        prelude::Entity,
        system::{In, ResMut, Resource},
        world::World,
    };

    use crate::render_graph_v2::RenderSystemIn;

    use super::RenderGraphV2;

    // cargo test -p bevy_render --lib -- render_graph_v2::tests::test_render_graph --nocapture
    #[test]
    fn test_render_graph() {
        let mut world = World::new();
        let mut graph = RenderGraphV2 {
            systems: Vec::new(),
        };

        #[derive(Resource)]
        struct Foo {
            bar: u32,
        }

        world.insert_resource(Foo { bar: 42 });

        fn main_node(In(view_entity): In<RenderSystemIn>, mut foo: ResMut<Foo>) {
            println!("main {view_entity:?} foo: {}", foo.bar);
            assert!(foo.bar == 42);
            foo.bar = 69;
        }

        fn end_post_process(In(view_entity): In<RenderSystemIn>, mut foo: ResMut<Foo>) {
            println!("end_post_process {view_entity:?} foo: {}", foo.bar);
            assert!(foo.bar == 69);
            foo.bar = 42;
        }

        graph.add_node(main_node);
        graph.add_node(end_post_process);

        graph.init(&mut world);

        graph.run(&mut world);

        graph.run(&mut world);
    }
}

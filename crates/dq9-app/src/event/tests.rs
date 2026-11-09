use super::*;

/// Home transform of a placement as `world_matrix` builds it (`0x02013168`): translation,
/// a `y` rotation and a non-uniform scale, so that a show that lost either would fail.
fn home() -> Mat4 {
    Mat4::from_translation(Vec3::new(10.0, 2.0, -3.0))
        * Mat4::from_rotation_y(0.5)
        * Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0))
}

fn scene_instance(world: Mat4) -> dq9_render::SceneInstance {
    dq9_render::SceneInstance {
        model: 0,
        world,
        home: home(),
        block: 3,
        instance: 7,
    }
}

fn placement() -> Placement {
    Placement::of(&scene_instance(home()))
}

#[test]
fn a_move_alone_does_not_show_a_placement_hidden_at_the_chosen_time() {
    let mut placement = Placement::of(&scene_instance(Mat4::from_scale(Vec3::ZERO)));
    let position = Vec3::new(1.0, 2.0, 3.0);
    placement.position = Some(position);
    assert_eq!(placement.world(), Mat4::from_scale(Vec3::ZERO));
    placement.hidden = false;
    assert_eq!(placement.world().w_axis.truncate(), position);
}

#[test]
fn placed_placement_keeps_rotation_and_scale_when_hidden_then_shown() {
    let mut placement = placement();
    let position = Vec3::new(-7.0, 0.5, 12.0);
    placement.position = Some(position);
    placement.hidden = true;
    assert_eq!(placement.world(), Mat4::from_scale(Vec3::ZERO));
    placement.hidden = false;
    let world = placement.world();
    assert_eq!(world.w_axis.truncate(), position);
    let home = home();
    assert_eq!(world.x_axis.truncate(), home.x_axis.truncate());
    assert_eq!(world.y_axis.truncate(), home.y_axis.truncate());
    assert_eq!(world.z_axis.truncate(), home.z_axis.truncate());
}

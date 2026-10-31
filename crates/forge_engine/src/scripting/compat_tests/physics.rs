//! Physics seen from scripts: gravity, contacts, constraints joined by attachments, welds, movers and explosions.

use crate::scripting::testkit::Game;

/// Runs `body` (which reports through a `report(text)` function) for `seconds` and returns what it reported.
fn run(body: &str, seconds: f32) -> (String, String) {
    let mut g = Game::new(&format!(
        r#"
        local reported = {{}}
        local function report(text) table.insert(reported, text) end
        local floor = Instance.new("Part", workspace)
        floor.Name = "Floor"
        floor.Anchored = true
        floor.Size = Vector3.new(200, 2, 200)
        floor.Position = Vector3.new(0, -1, 0)
        {body}
        game:GetService("ReplicatedStorage").Out.Value = table.concat(reported, "|")
    "#
    ));
    g.frames((seconds * 60.0) as usize + 4);
    (g.out(), g.logs())
}

#[test]
fn unanchored_parts_fall_land_and_touch() {
    let (out, logs) = run(
        r#"
        local ball = Instance.new("Part", workspace)
        ball.Shape = Enum.PartType.Ball
        ball.Size = Vector3.new(2, 2, 2)
        ball.Position = Vector3.new(0, 10, 0)
        local touched = nil
        ball.Touched:Connect(function(other) touched = touched or other.Name end)
        local ended = false
        ball.TouchEnded:Connect(function() ended = true end)
        task.wait(2)
        report("rest:" .. string.format("%.1f", ball.Position.Y))
        report("touched:" .. tostring(touched))
        report("velocity:" .. string.format("%.1f", ball.AssemblyLinearVelocity.Magnitude))
        report("gravity:" .. string.format("%.1f", workspace.Gravity))
        ball.AssemblyLinearVelocity = Vector3.new(0, 50, 0)
        task.wait(0.3)
        report("thrown:" .. tostring(ball.Position.Y > 3) .. "," .. tostring(ended))
    "#,
        3.0,
    );
    assert_eq!(out, "rest:1.0|touched:Floor|velocity:0.0|gravity:196.2|thrown:true,true", "{logs}");
}

#[test]
fn anchored_parts_do_not_move_and_cancollide_off_lets_things_through() {
    let (out, logs) = run(
        r#"
        local held = Instance.new("Part", workspace)
        held.Anchored = true
        held.Position = Vector3.new(5, 10, 0)
        local ghost = Instance.new("Part", workspace)
        ghost.CanCollide = false
        ghost.Position = Vector3.new(-5, 5, 0)
        local touched = false
        ghost.Touched:Connect(function() touched = true end)
        task.wait(1)
        report("anchored:" .. held.Position.Y)
        report("ghost fell through:" .. tostring(ghost.Position.Y < -2))
        report("ghost still touched:" .. tostring(touched))
        local quiet = Instance.new("Part", workspace)
        quiet.CanTouch = false
        quiet.Position = Vector3.new(0, 3, 0)
        local fired = false
        quiet.Touched:Connect(function() fired = true end)
        task.wait(1)
        report("CanTouch off:" .. tostring(fired))
    "#,
        3.0,
    );
    assert_eq!(out, "anchored:10|ghost fell through:true|ghost still touched:true|CanTouch off:false", "{logs}");
}

#[test]
fn weld_constraints_keep_parts_together_and_break_when_destroyed() {
    let (out, logs) = run(
        r#"
        local a = Instance.new("Part", workspace)
        a.Position = Vector3.new(0, 10, 0)
        local b = Instance.new("Part", workspace)
        b.Position = Vector3.new(4, 10, 0)
        local weld = Instance.new("WeldConstraint")
        weld.Part0, weld.Part1 = a, b
        weld.Parent = a
        report("class:" .. weld.ClassName .. "," .. tostring(weld.Enabled))
        task.wait(2)
        report("together:" .. string.format("%.1f", b.Position.X - a.Position.X))
        report("level:" .. tostring(math.abs(b.Position.Y - a.Position.Y) < 0.05))
        report("landed:" .. string.format("%.1f", a.Position.Y))
        weld:Destroy()
        b.AssemblyLinearVelocity = Vector3.new(20, 0, 0)
        task.wait(0.5)
        report("separate:" .. tostring(b.Position.X - a.Position.X > 5))
    "#,
        4.0,
    );
    assert_eq!(out, "class:WeldConstraint,true|together:4.0|level:true|landed:0.5|separate:true", "{logs}");
}

#[test]
fn old_style_welds_and_motors_join_by_their_c0_and_c1() {
    let (out, logs) = run(
        r#"
        local a = Instance.new("Part", workspace)
        a.Anchored = true
        a.Position = Vector3.new(0, 10, 0)
        local b = Instance.new("Part", workspace)
        b.Position = Vector3.new(0, 5, 0)
        local weld = Instance.new("Weld")
        weld.Part0, weld.Part1 = a, b
        weld.C0 = CFrame.new(0, -3, 0)
        weld.Parent = a
        task.wait(1)
        report("weld holds it at C0:" .. string.format("%.1f", b.Position.Y))
        local motor = Instance.new("Motor6D")
        report("motor:" .. motor.ClassName .. "," .. tostring(motor:IsA("JointInstance")) .. "," .. typeof(motor.Transform))
        local manual = Instance.new("ManualWeld")
        report("manual:" .. tostring(manual:IsA("JointInstance")))
    "#,
        2.0,
    );
    assert_eq!(out, "weld holds it at C0:7.0|motor:Motor6D,true,CFrame|manual:true", "{logs}");
}

#[test]
fn constraints_join_parts_by_their_attachments() {
    let (out, logs) = run(
        r#"
        local anchor = Instance.new("Part", workspace)
        anchor.Anchored = true
        anchor.Position = Vector3.new(0, 20, 0)
        local a0 = Instance.new("Attachment", anchor)
        a0.Position = Vector3.new(0, -1, 0)

        local weight = Instance.new("Part", workspace)
        weight.Position = Vector3.new(0, 14, 0)
        local a1 = Instance.new("Attachment", weight)

        local rope = Instance.new("RopeConstraint")
        rope.Attachment0, rope.Attachment1 = a0, a1
        rope.Length = 5
        rope.Parent = anchor
        report("rope:" .. rope.ClassName .. "," .. rope.Length .. "," .. tostring(rope.Attachment0 == a0))
        task.wait(3)
        local hang = (a0.WorldPosition - a1.WorldPosition).Magnitude
        report("hangs at its length:" .. tostring(hang > 4.5 and hang < 5.6) .. string.format("(%.2f)", hang))

        local rod_anchor = Instance.new("Part", workspace)
        rod_anchor.Anchored = true
        rod_anchor.Position = Vector3.new(30, 20, 0)
        local r0 = Instance.new("Attachment", rod_anchor)
        local bob = Instance.new("Part", workspace)
        bob.Position = Vector3.new(30, 16, 0)
        local r1 = Instance.new("Attachment", bob)
        local rod = Instance.new("RodConstraint")
        rod.Attachment0, rod.Attachment1 = r0, r1
        rod.Length = 4
        rod.Parent = rod_anchor
        task.wait(2)
        local rod_length = (r0.WorldPosition - r1.WorldPosition).Magnitude
        report("rod stays rigid:" .. tostring(math.abs(rod_length - 4) < 0.3))
    "#,
        6.0,
    );
    assert!(out.starts_with("rope:RopeConstraint,5,true|hangs at its length:true"), "{out}\n{logs}");
    assert!(out.ends_with("rod stays rigid:true"), "{out}\n{logs}");
}

#[test]
fn hinges_spin_with_their_motor_and_springs_hold_a_length() {
    let (out, logs) = run(
        r#"
        local base = Instance.new("Part", workspace)
        base.Anchored = true
        base.Position = Vector3.new(0, 10, 0)
        local wheel = Instance.new("Part", workspace)
        wheel.Position = Vector3.new(0, 10, 0)
        wheel.Size = Vector3.new(4, 4, 1)
        local b0 = Instance.new("Attachment", base)
        local w0 = Instance.new("Attachment", wheel)
        local hinge = Instance.new("HingeConstraint")
        hinge.Attachment0, hinge.Attachment1 = b0, w0
        hinge.ActuatorType = Enum.ActuatorType.Motor
        hinge.MotorMaxTorque = 100000
        hinge.AngularVelocity = 4
        hinge.Parent = base
        report("hinge:" .. hinge.ClassName .. "," .. hinge.ActuatorType.Name)
        task.wait(1.5)
        report("spins:" .. tostring(wheel.AssemblyAngularVelocity.Magnitude > 1))

        local top = Instance.new("Part", workspace)
        top.Anchored = true
        top.Position = Vector3.new(30, 25, 0)
        local t0 = Instance.new("Attachment", top)
        local ball = Instance.new("Part", workspace)
        ball.Position = Vector3.new(30, 15, 0)
        local t1 = Instance.new("Attachment", ball)
        local spring = Instance.new("SpringConstraint")
        spring.Attachment0, spring.Attachment1 = t0, t1
        spring.FreeLength = 6
        spring.Stiffness = 2000
        spring.Damping = 200
        spring.Parent = top
        task.wait(3)
        local length = (t0.WorldPosition - t1.WorldPosition).Magnitude
        report("spring rests beyond its free length:" .. tostring(length > 6 and length < 12))
        report("current length:" .. tostring(math.abs(spring.CurrentLength - length) < 0.5))
    "#,
        5.0,
    );
    assert_eq!(out, "hinge:HingeConstraint,Motor|spins:true|spring rests beyond its free length:true|current length:true", "{logs}");
}

#[test]
fn no_collision_constraints_let_two_parts_pass_through_each_other() {
    let (out, logs) = run(
        r#"
        local function crate(x, y)
            local p = Instance.new("Part", workspace)
            p.Size = Vector3.new(4, 4, 4)
            p.Position = Vector3.new(x, y, 0)
            return p
        end
        local below, above = crate(0, 2), crate(0, 8)
        below.Anchored = true
        local nc = Instance.new("NoCollisionConstraint")
        nc.Part0, nc.Part1 = below, above
        nc.Parent = below
        task.wait(1.5)
        report("fell through:" .. tostring(above.Position.Y < 4))

        local base, top = crate(30, 2), crate(30, 8)
        base.Anchored = true
        task.wait(1.5)
        report("collides:" .. tostring(top.Position.Y > 3.5))
    "#,
        4.0,
    );
    assert_eq!(out, "fell through:true|collides:true", "{logs}");
}

#[test]
fn movers_push_parts_the_way_their_classes_document() {
    let (out, logs) = run(
        r#"
        local function free(x)
            local p = Instance.new("Part", workspace)
            p.Position = Vector3.new(x, 30, 0)
            return p
        end
        local a = free(0)
        local bv = Instance.new("BodyVelocity")
        bv.Velocity = Vector3.new(10, 0, 0)
        bv.MaxForce = Vector3.new(1e6, 1e6, 1e6)
        bv.Parent = a
        local b = free(20)
        local lv = Instance.new("LinearVelocity")
        local att = Instance.new("Attachment", b)
        lv.Attachment0 = att
        lv.VectorVelocity = Vector3.new(0, 0, 10)
        lv.MaxForce = 1e6
        lv.Parent = b
        local c = free(40)
        local force = Instance.new("VectorForce")
        local catt = Instance.new("Attachment", c)
        force.Attachment0 = catt
        force.RelativeTo = Enum.ActuatorRelativeTo.World
        force.Force = Vector3.new(0, c.AssemblyMass * workspace.Gravity * 2, 0)
        force.Parent = c
        local d = free(60)
        local bp = Instance.new("BodyPosition")
        bp.Position = Vector3.new(60, 40, 0)
        bp.MaxForce = Vector3.new(1e6, 1e6, 1e6)
        bp.Parent = d
        task.wait(1)
        report("BodyVelocity:" .. tostring(a.Position.X > 7))
        report("LinearVelocity:" .. tostring(b.Position.Z > 7))
        report("VectorForce lifts:" .. tostring(c.Position.Y > 31))
        report("BodyPosition:" .. tostring(math.abs(d.Position.Y - 40) < 3))
        bv:Destroy()
        report("removed:" .. tostring(bv.Parent == nil))
    "#,
        2.0,
    );
    assert_eq!(out, "BodyVelocity:true|LinearVelocity:true|VectorForce lifts:true|BodyPosition:true|removed:true", "{logs}");
}

#[test]
fn explosions_push_parts_and_report_who_they_hit() {
    let (out, logs) = run(
        r#"
        local near = Instance.new("Part", workspace)
        near.Position = Vector3.new(3, 1.5, 0)
        local far = Instance.new("Part", workspace)
        far.Position = Vector3.new(80, 1.5, 0)
        local anchored = Instance.new("Part", workspace)
        anchored.Anchored = true
        anchored.Position = Vector3.new(0, 1.5, 3)
        task.wait(0.2)
        local boom = Instance.new("Explosion")
        boom.Position = Vector3.new(0, 1, 0)
        boom.BlastRadius = 10
        boom.BlastPressure = 500000
        boom.DestroyJointRadiusPercent = 0
        local hit = {}
        boom.Hit:Connect(function(part, distance) hit[part.Name] = distance end)
        near.Name, far.Name, anchored.Name = "Near", "Far", "Anchored"
        boom.Parent = workspace
        task.wait(0.5)
        report("near moved:" .. tostring(near.Position.X > 4 or near.AssemblyLinearVelocity.Magnitude > 1))
        report("far untouched:" .. tostring(math.abs(far.Position.X - 80) < 0.1))
        report("Hit near:" .. tostring(hit.Near ~= nil and hit.Near < 5))
        report("Hit far:" .. tostring(hit.Far))
        report("Hit anchored:" .. tostring(hit.Anchored ~= nil))
        report("anchored stays:" .. tostring(anchored.Position.Z == 3))
    "#,
        2.0,
    );
    assert_eq!(out, "near moved:true|far untouched:true|Hit near:true|Hit far:nil|Hit anchored:true|anchored stays:true", "{logs}");
}

#[test]
fn collision_groups_decide_who_collides_with_whom() {
    let (out, logs) = run(
        r#"
        local physics = game:GetService("PhysicsService")
        physics:RegisterCollisionGroup("Ghosts")
        physics:CollisionGroupSetCollidable("Ghosts", "Default", false)
        report("registered:" .. tostring(physics:IsCollisionGroupRegistered("Ghosts")))
        report("collidable:" .. tostring(physics:CollisionGroupsAreCollidable("Ghosts", "Default")) .. "," .. tostring(physics:CollisionGroupsAreCollidable("Ghosts", "Ghosts")))
        local ghost = Instance.new("Part", workspace)
        ghost.CollisionGroup = "Ghosts"
        ghost.Position = Vector3.new(0, 8, 0)
        local solid = Instance.new("Part", workspace)
        solid.Position = Vector3.new(30, 8, 0)
        task.wait(2)
        report("ghost falls through the floor:" .. tostring(ghost.Position.Y < -3))
        report("solid rests:" .. tostring(math.abs(solid.Position.Y - 0.5) < 0.3))
        report("groups:" .. tostring(#physics:GetRegisteredCollisionGroups() >= 2))
    "#,
        3.0,
    );
    assert_eq!(
        out,
        "registered:true|collidable:false,true|ghost falls through the floor:true|solid rests:true|groups:true",
        "{logs}"
    );
}

#[test]
fn a_scripted_player_body_walks_jumps_and_stays_upright() {
    // The pieces a custom player controller is made of: a part with FixedRotation, its velocity, and a ray to find the ground.
    let (out, logs) = run(
        r#"
        local floor = Instance.new("Part", workspace)
        floor.Anchored = true
        floor.Size = Vector3.new(200, 2, 200)
        floor.Position = Vector3.new(0, -1, 0)
        local body = Instance.new("Part", workspace)
        body.Size = Vector3.new(2, 5, 2)
        body.Position = Vector3.new(0, 4, 0)
        body.FixedRotation = true
        body.Friction = 0
        local params = RaycastParams.new()
        params.FilterDescendantsInstances = { body }
        local function grounded()
            return workspace:Raycast(body.Position, Vector3.new(0, -(body.Size.Y / 2 + 0.2), 0), params) ~= nil
        end
        task.wait(1)
        report("grounded:" .. tostring(grounded()))
        local x0 = body.Position.X
        body.AssemblyLinearVelocity = Vector3.new(16, body.AssemblyLinearVelocity.Y, 0)
        task.wait(1)
        report("walks:" .. tostring(body.Position.X - x0 > 10))
        body.AssemblyLinearVelocity = Vector3.new(0, 60, 0)
        local peak = body.Position.Y
        for _ = 1, 30 do
            task.wait()
            peak = math.max(peak, body.Position.Y)
        end
        report("jumps:" .. tostring(peak > 8))
        -- Something knocks into it from the side: it must not tip over.
        local ball = Instance.new("Part", workspace)
        ball.Shape = Enum.PartType.Ball
        ball.Size = Vector3.new(4, 4, 4)
        ball.Position = body.Position + Vector3.new(-6, 1.5, 0.8)
        ball.AssemblyLinearVelocity = Vector3.new(60, 0, 0)
        task.wait(1.5)
        local up = body.CFrame.UpVector
        report("upright:" .. tostring(up.Y > 0.999))
    "#,
        6.0,
    );
    assert_eq!(out, "grounded:true|walks:true|jumps:true|upright:true", "{logs}");
}
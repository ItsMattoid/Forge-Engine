use crate::scripting::testkit::{checks_for, Game};

#[test]
fn connections_once_wait_and_disconnect() {
    checks_for(
        r#"
        local event = Instance.new("BindableEvent")
        local calls = {}
        local connection = event.Event:Connect(function(a, b) table.insert(calls, tostring(a) .. ":" .. tostring(b)) end)
        equal("Connected", connection.Connected, true)
        equal("typeof connection", typeof(connection), "RBXScriptConnection")
        equal("typeof signal", typeof(event.Event), "RBXScriptSignal")
        event:Fire(1, 2)
        event:Fire("x")
        task.wait()
        equal("handlers get the arguments", table.concat(calls, ","), "1:2,x:nil")
        connection:Disconnect()
        equal("Disconnected", connection.Connected, false)
        event:Fire(3)
        task.wait()
        equal("no calls after Disconnect", #calls, 2)
        connection:Disconnect() -- (twice is harmless)

        local once = 0
        event.Event:Once(function() once += 1 end)
        event:Fire()
        event:Fire()
        task.wait()
        equal("Once fires once", once, 1)

        task.delay(0.05, function() event:Fire("later", 2) end)
        local a, b = event.Event:Wait()
        equal("Wait returns the arguments", a .. b, "later2")

        -- Handlers run in order of connection, and a handler may disconnect another.
        local order = {}
        local c1, c2
        c1 = event.Event:Connect(function() table.insert(order, 1) c2:Disconnect() end)
        c2 = event.Event:Connect(function() table.insert(order, 2) end)
        event.Event:Connect(function() table.insert(order, 3) end)
        event:Fire()
        task.wait()
        equal("order", table.concat(order, ","), "1,3")
        throws("Connect needs a function", function() event.Event:Connect(5) end)

        -- Handlers may yield.
        local woke = false
        event.Event:Connect(function() task.wait(0.05) woke = true end)
        event:Fire()
        task.wait(0.2)
        equal("a handler that waits resumes", woke, true)
        check("ConnectParallel exists", event.Event.ConnectParallel ~= nil)
    "#,
        60,
    );
}

#[test]
fn errors_in_one_handler_do_not_stop_the_others_and_reach_the_log() {
    let mut g = Game::new(
        r#"
        local event = Instance.new("BindableEvent")
        local ran = {}
        event.Event:Connect(function() error("boom from a handler") end)
        event.Event:Connect(function() table.insert(ran, "second") end)
        local errors = {}
        game:GetService("ScriptContext").Error:Connect(function(message, trace, source) table.insert(errors, message .. "|" .. tostring(source)) end)
        local lines = {}
        game:GetService("LogService").MessageOut:Connect(function(message, kind) table.insert(lines, tostring(kind) .. ":" .. message) end)
        event:Fire()
        print("hello", 1, true)
        warn("careful")
        task.wait()
        task.wait()
        local joined = table.concat(lines, "\n")
        game:GetService("ReplicatedStorage").Out.Value = table.concat({
            #ran == 1 and "ran" or "stopped",
            #errors >= 1 and string.find(errors[1], "boom from a handler", 1, true) and "error-reported" or "error-missed",
            string.find(joined, "Enum.MessageType.MessageOutput:hello 1 true", 1, true) and "print" or "no-print",
            string.find(joined, "Enum.MessageType.MessageWarning:careful", 1, true) and "warn" or "no-warn",
            string.find(joined, "Enum.MessageType.MessageError:", 1, true) and "error-line" or "no-error-line",
        }, ",")
    "#,
    );
    g.frames(6);
    assert_eq!(g.out(), "ran,error-reported,print,warn,error-line", "{}", g.logs());
}

#[test]
fn task_library_and_the_older_global_functions() {
    checks_for(
        r#"
        local log = {}
        local function note(x) table.insert(log, x) end

        task.spawn(function(a, b) note("spawn " .. a .. b) end, "x", "y")
        note("after spawn")
        task.defer(function() note("deferred") end)
        note("after defer")
        task.delay(0.1, function(who) note("delay " .. who) end, "me")
        task.wait()
        task.wait()
        local waited = task.wait(0.2)
        check("task.wait returns the time waited", waited >= 0.19 and waited < 0.5, waited)
        equal("order", table.concat(log, "|"), "spawn xy|after spawn|after defer|deferred|delay me")

        -- Cancelling.
        local ran = false
        local thread = task.delay(0.1, function() ran = true end)
        task.cancel(thread)
        local looping = 0
        local loop = task.spawn(function() while true do looping += 1 task.wait() end end)
        task.wait(0.1)
        task.cancel(loop)
        local count = looping
        task.wait(0.1)
        task.wait(0.2)
        equal("a cancelled delay never runs", ran, false)
        equal("a cancelled loop stops", looping, count)

        -- task.spawn takes a thread, too.
        local co = coroutine.create(function(v) note("thread " .. v) end)
        task.spawn(co, "ok")
        equal("spawned thread", log[#log], "thread ok")

        -- The older functions.
        local legacy = {}
        spawn(function() table.insert(legacy, "spawn") end)
        delay(0.05, function() table.insert(legacy, "delay") end)
        local elapsed, total = wait(0.1)
        check("wait returns two numbers", type(elapsed) == "number" and type(total) == "number")
        equal("legacy functions ran", table.concat(legacy, ","), "spawn,delay")
        check("tick", tick() > 1.6e9)
        check("time advances", time() > 0)
        check("elapsedTime", elapsedTime() >= 0)
        check("os.clock", os.clock() >= 0)
        check("version", type(version()) == "string")
        check("settings", settings().Rendering ~= nil and settings().Physics ~= nil)
        check("UserSettings", UserSettings():GetService("UserGameSettings").MasterVolume ~= nil)
        check("stats", stats() ~= nil)
        debug.profilebegin("x")
        debug.profileend()
        check("task.synchronize", pcall(task.synchronize) and pcall(task.desynchronize))
    "#,
        120,
    );
}

#[test]
fn bindable_and_remote_objects_loop_back() {
    checks_for(
        r#"
        -- BindableFunction
        local fn = Instance.new("BindableFunction")
        fn.OnInvoke = function(a, b) return a + b, "sum" end
        local sum, label = fn:Invoke(2, 3)
        check("Invoke returns what OnInvoke returns", sum == 5 and label == "sum")
        fn.OnInvoke = nil
        throws("Invoke with no callback", function() fn:Invoke() end)
        throws("OnInvoke must be a function", function() fn.OnInvoke = 5 end)

        -- RemoteEvent: the "server" sees the local player first.
        local remote = Instance.new("RemoteEvent")
        local got = {}
        remote.OnServerEvent:Connect(function(player, a) got.player = player got.a = a end)
        remote.OnClientEvent:Connect(function(a, b) got.client = tostring(a) .. tostring(b) end)
        remote:FireServer("hello")
        remote:FireClient(game:GetService("Players").LocalPlayer, 1, 2)
        task.wait()
        equal("OnServerEvent gets the player", got.player, game:GetService("Players").LocalPlayer)
        equal("and the arguments", got.a, "hello")
        equal("OnClientEvent", got.client, "12")
        got.client = nil
        remote:FireAllClients("x", "y")
        equal("FireAllClients", got.client, "xy")
        throws("FireClient needs a player", function() remote:FireClient(workspace) end)
        check("unreliable", Instance.new("UnreliableRemoteEvent").OnServerEvent ~= nil)

        -- RemoteFunction
        local rf = Instance.new("RemoteFunction")
        rf.OnServerInvoke = function(player, x) return player.Name .. x end
        rf.OnClientInvoke = function(x) return "client" .. x end
        equal("InvokeServer", rf:InvokeServer("!"), game:GetService("Players").LocalPlayer.Name .. "!")
        equal("InvokeClient", rf:InvokeClient(game:GetService("Players").LocalPlayer, 7), "client7")
        local d = Instance.new("RemoteFunction")
        throws("InvokeServer with no callback", function() d:InvokeServer() end)
    "#,
        20,
    );
}

#[test]
fn run_service_signals_sides_and_render_steps() {
    checks_for(
        r#"
        local run = game:GetService("RunService")
        equal("IsRunning", run:IsRunning(), true)
        equal("IsStudio", run:IsStudio(), false)
        equal("IsEdit", run:IsEdit(), false)
        equal("a Script is the server", run:IsServer(), true)
        equal("and not the client", run:IsClient(), false)

        local seen = {}
        for _, name in { "Heartbeat", "Stepped", "RenderStepped", "PreRender", "PreAnimation", "PreSimulation", "PostSimulation" } do
            run[name]:Connect(function(...) seen[name] = (seen[name] or 0) + 1 end)
        end
        local dt = run.Heartbeat:Wait()
        check("Heartbeat passes the frame time", type(dt) == "number" and dt > 0 and dt < 0.1)
        local t, step = run.Stepped:Wait()
        check("Stepped passes (time, dt)", type(t) == "number" and type(step) == "number")
        task.wait(0.1)
        for name, count in seen do
            check(name .. " fired every frame", count >= 5, count)
        end
        for _, name in { "Heartbeat", "Stepped", "RenderStepped", "PreRender", "PreAnimation", "PreSimulation", "PostSimulation" } do
            check(name .. " exists", seen[name] ~= nil)
        end

        local order = {}
        run:BindToRenderStep("late", Enum.RenderPriority.Last.Value, function() table.insert(order, "late") end)
        run:BindToRenderStep("camera", Enum.RenderPriority.Camera.Value, function() table.insert(order, "camera") end)
        run:BindToRenderStep("first", Enum.RenderPriority.First.Value, function(dt) table.insert(order, "first") end)
        task.wait()
        task.wait()
        check("steps run in priority order each frame", table.concat(order, ",", 1, 3) == "first,camera,late", table.concat(order, ","))
        run:UnbindFromRenderStep("camera")
        order = {}
        task.wait()
        task.wait()
        check("an unbound step stops", not table.find(order, "camera"))
        throws("a name binds once", function() run:BindToRenderStep("first", 1, function() end) end)
        equal("RenderPriority numbers", Enum.RenderPriority.Camera.Value, 200)
    "#,
        60,
    );
}

#[test]
fn a_local_script_is_the_client_and_a_module_is_both() {
    let mut g = Game::with_files(&[
        ("client.luau", "local run = game:GetService('RunService') game:GetService('ReplicatedStorage').Client.Value = tostring(run:IsClient()) .. tostring(run:IsServer())"),
        ("server.luau", "local run = game:GetService('RunService') game:GetService('ReplicatedStorage').Server.Value = tostring(run:IsClient()) .. tostring(run:IsServer())"),
        ("module.luau", "local run = game:GetService('RunService') return tostring(run:IsClient()) .. tostring(run:IsServer())"),
        ("both.luau", "game:GetService('ReplicatedStorage').Module.Value = require(script.Parent.Shared)"),
        ("ctx.luau", "game:GetService('ReplicatedStorage').Context.Value = tostring(game:GetService('RunService'):IsClient())"),
    ]);
    {
        let mut dm = g.dm.borrow_mut();
        let rs = dm.service("ReplicatedStorage").unwrap();
        for name in ["Client", "Server", "Module", "Context"] {
            let v = dm.create("StringValue").unwrap();
            dm.set_name(v, name);
            dm.set_parent(v, Some(rs)).unwrap();
        }
    }
    g.add_script("Client", "StarterPlayer", "client.luau", "LocalScript");
    g.add_script("Server", "ServerScriptService", "server.luau", "Script");
    g.add_script("Shared", "ServerScriptService", "module.luau", "ModuleScript");
    g.add_script("Both", "ServerScriptService", "both.luau", "Script");
    g.add_script("Ctx", "ServerScriptService", "ctx.luau", "Script");
    {
        // A Script whose RunContext is Client counts as a client script.
        let mut dm = g.dm.borrow_mut();
        let sss = dm.service("ServerScriptService").unwrap();
        let ctx = dm.find_child(sss, "Ctx").unwrap();
        dm.set_prop(ctx, "RunContext", crate::core::Value::Enum("Client".into())).unwrap();
    }
    g.host.start_all();
    g.frames(3);
    let read = |name: &str| {
        let dm = g.dm.borrow();
        let rs = dm.service("ReplicatedStorage").unwrap();
        dm.string(dm.find_child(rs, name).unwrap(), "Value")
    };
    assert_eq!(read("Client"), "truefalse", "a LocalScript is a client script");
    assert_eq!(read("Server"), "falsetrue", "a Script is a server script");
    assert_eq!(read("Module"), "truetrue", "a ModuleScript answers for both");
    assert_eq!(read("Context"), "true", "RunContext Client makes a Script a client script");
}
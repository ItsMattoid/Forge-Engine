use crate::{core::Value, scripting::testkit::Game};

/// A game with `files` and, for each `(name, file, class)`, a script object in ServerScriptService; `Out` is the output.
fn game(files: &[(&str, &str)], scripts: &[(&str, &str, &str)]) -> Game {
    let mut g = Game::with_files(files);
    for (name, file, class) in scripts {
        g.add_script(name, "ServerScriptService", file, class);
    }
    g.host.start_all();
    g
}

#[test]
fn require_runs_a_module_once_and_hands_everybody_the_same_value() {
    let mut g = game(
        &[
            ("counter.luau", "local Counter = { count = 0 } Counter.runs = (_G.module_runs or 0) + 1 _G.module_runs = Counter.runs function Counter.bump() Counter.count += 1 return Counter.count end return Counter"),
            ("a.luau", "local c = require(script.Parent.Counter) c.bump() _G.a = c"),
            ("b.luau", "local c = require(script.Parent.Counter) c.bump() game:GetService('ReplicatedStorage').Out.Value = table.concat({ c.count, c.runs, tostring(c == _G.a), typeof(c) }, ',')"),
        ],
        &[("Counter", "counter.luau", "ModuleScript"), ("A", "a.luau", "Script"), ("B", "b.luau", "Script")],
    );
    g.frames(4);
    assert_eq!(g.out(), "2,1,true,table", "{}", g.logs());
}

#[test]
fn modules_can_be_classes_and_wait_for_their_own_work() {
    let mut g = game(
        &[
            (
                "account.luau",
                r#"
                local Account = {}
                Account.__index = Account
                function Account.new(owner, balance)
                    return setmetatable({ owner = owner, balance = balance or 0 }, Account)
                end
                function Account:deposit(n)
                    assert(n > 0, "deposit must be positive")
                    self.balance += n
                    return self
                end
                task.wait(0.05) -- a module may yield while it loads
                return Account
            "#,
            ),
            (
                "main.luau",
                r#"
                local Account = require(script.Parent.Account)
                local acc = Account.new("ann", 10):deposit(5):deposit(10)
                local ok, err = pcall(function() acc:deposit(-1) end)
                local out = game:GetService("ReplicatedStorage").Out
                out.Value = table.concat({ acc.balance, acc.owner, tostring(ok), string.find(tostring(err), "deposit must be positive", 1, true) and "asserted" or "?" }, ",")
            "#,
            ),
        ],
        &[("Account", "account.luau", "ModuleScript"), ("Main", "main.luau", "Script")],
    );
    g.frames(10);
    assert_eq!(g.out(), "25,ann,false,asserted", "{}", g.logs());
}

#[test]
fn two_scripts_requiring_a_slow_module_both_get_it() {
    let mut g = game(
        &[
            ("slow.luau", "task.wait(0.1) _G.loads = (_G.loads or 0) + 1 return { name = 'slow' }"),
            ("first.luau", "_G.first = require(script.Parent.Slow).name"),
            ("second.luau", "task.wait(0.02) _G.second = require(script.Parent.Slow).name task.wait(0.05) game:GetService('ReplicatedStorage').Out.Value = table.concat({ _G.first or '-', _G.second, _G.loads }, ',')"),
        ],
        &[("Slow", "slow.luau", "ModuleScript"), ("First", "first.luau", "Script"), ("Second", "second.luau", "Script")],
    );
    g.frames(30);
    assert_eq!(g.out(), "slow,slow,1", "the module ran once, and the script that came second waited for it: {}", g.logs());
}

#[test]
fn require_reports_what_roblox_reports() {
    let mut g = game(
        &[
            ("a.luau", "return require(script.Parent.B)"),
            ("b.luau", "return require(script.Parent.A)"),
            ("nothing.luau", "local x = 1"),
            ("two.luau", "return 1, 2"),
            ("broken.luau", "error('module failed on purpose')"),
            (
                "main.luau",
                r#"
                local function try(f)
                    local ok, err = pcall(f)
                    return ok and "ok" or (tostring(err):gsub("\n.*", ""))
                end
                local p = script.Parent
                local lines = {
                    try(function() return require(p.A) end),
                    try(function() return require(p.Nothing) end),
                    try(function() return require(p.Two) end),
                    try(function() return require(p.Broken) end),
                    try(function() return require(p.Broken) end),
                    try(function() return require(5) end),
                    try(function() return require(p.Main) end),
                    try(function() return require(workspace) end),
                }
                game:GetService("ReplicatedStorage").Out.Value = table.concat(lines, "\n")
            "#,
            ),
        ],
        &[("A", "a.luau", "ModuleScript"), ("B", "b.luau", "ModuleScript"), ("Nothing", "nothing.luau", "ModuleScript"), ("Two", "two.luau", "ModuleScript"), ("Broken", "broken.luau", "ModuleScript"), ("Main", "main.luau", "Script")],
    );
    g.frames(4);
    let out = g.out();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "{out}");
    assert!(lines[0].contains("required recursively"), "a cycle: {}", lines[0]);
    assert!(lines[1].contains("did not return exactly one value"), "a module that returns nothing: {}", lines[1]);
    assert!(lines[2].contains("did not return exactly one value"), "a module that returns two values: {}", lines[2]);
    assert!(lines[3].contains("module failed on purpose"), "an error in a module reaches the first caller: {}", lines[3]);
    assert!(lines[4].contains("experienced an error while loading"), "and later callers are told it failed: {}", lines[4]);
    assert!(lines[5].contains("invalid argument"), "not a module: {}", lines[5]);
    assert!(lines[6].contains("not a ModuleScript"), "a Script is not a module: {}", lines[6]);
    assert!(lines[7].contains("invalid argument"), "a Workspace is not a module: {}", lines[7]);
}

#[test]
fn a_module_sees_its_own_script_and_a_clone_is_a_different_module() {
    let mut g = game(
        &[
            ("who.luau", "return { name = script.Name, parent = script.Parent.Name, full = script:GetFullName() }"),
            ("main.luau", "local p = script.Parent local a = require(p.Who) local copy = p.Who:Clone() copy.Name = 'Copy' copy.Parent = p local b = require(copy) game:GetService('ReplicatedStorage').Out.Value = table.concat({ a.name, a.parent, a.full, b.name, tostring(a == b) }, ',')"),
        ],
        &[("Who", "who.luau", "ModuleScript"), ("Main", "main.luau", "Script")],
    );
    g.frames(4);
    assert_eq!(g.out(), "Who,ServerScriptService,ServerScriptService.Who,Copy,false", "{}", g.logs());
}

#[test]
fn global_and_shared_tables_are_writable_and_shared_between_scripts() {
    let mut g = game(
        &[
            ("a.luau", "_G.score = 10 shared.config = { speed = 3 } game:GetService('ReplicatedStorage').Out.Value = 'set'"),
            ("b.luau", "task.wait(0.05) game:GetService('ReplicatedStorage').Out.Value = table.concat({ _G.score, shared.config.speed, type(_G), type(shared) }, ',')"),
        ],
        &[("A", "a.luau", "Script"), ("B", "b.luau", "Script")],
    );
    g.frames(10);
    assert_eq!(g.out(), "10,3,table,table", "{}", g.logs());
    // A global a script makes is its own: the other scripts do not see it (only `_G` and `shared` are shared).
    let mut g = game(
        &[("c.luau", "myglobal = 5 print = nil _G.c = myglobal"), ("d.luau", "task.wait(0.05) game:GetService('ReplicatedStorage').Out.Value = tostring(myglobal) .. type(_G.c) .. type(print)")],
        &[("C", "c.luau", "Script"), ("D", "d.luau", "Script")],
    );
    g.frames(10);
    assert_eq!(g.out(), "nilnumberfunction", "a script's globals are private to it: {}", g.logs());
}

#[test]
fn script_context_comes_from_the_script_object() {
    let mut g = game(
        &[("s.luau", "local out = game:GetService('ReplicatedStorage').Out out.Value = table.concat({ script.Name, script.ClassName, tostring(script.Enabled), tostring(script.Disabled), script.Parent.Name, tostring(script:IsA('BaseScript')) }, ',')")],
        &[("Probe", "s.luau", "Script")],
    );
    g.frames(3);
    assert_eq!(g.out(), "Probe,Script,true,false,ServerScriptService,true", "{}", g.logs());
    // Disabled is the opposite of Enabled, both ways.
    let dm = g.dm.clone();
    let mut dm = dm.borrow_mut();
    let sss = dm.service("ServerScriptService").unwrap();
    let probe = dm.find_child(sss, "Probe").unwrap();
    dm.set_prop(probe, "Enabled", Value::Bool(false)).unwrap();
    assert_eq!(dm.get_prop(probe, "Disabled"), Some(Value::Bool(true)));
    dm.set_prop(probe, "Disabled", Value::Bool(false)).unwrap();
    assert_eq!(dm.get_prop(probe, "Enabled"), Some(Value::Bool(true)));
}
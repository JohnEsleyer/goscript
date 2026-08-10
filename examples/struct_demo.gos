// struct_demo.go --- structs, shared references, and for loops
// Thin slices use shared references (rc/RefCell), so two variables can
// alias the same struct and mutations show through both.
// Run it with:  cargo run -- examples/struct_demo.go

type Transform struct {
    X float64
    Y float64
}

type Player struct {
    name string
    hp int
}

var transform = Transform{X: 0, Y: 0}
var player = Player{name: "Hero", hp: 100}

var sentry = player

func Move(dx float64, dy float64) {
    transform.X = transform.X + dx
    transform.Y = transform.Y + dy
    Log("moved to", transform)
}

func Damage(n int) {
    player.hp = player.hp - n
    Log("player hp after hit:", player.hp)
}

Log("sentry alias before hit:", sentry.hp)
Damage(35)

var total = 0
for i := 0; i < 10; i = i + 1 {
    total = total + i
    if i == 2 {
        continue
    }
}
Log("sum 0..9 with continue:", total)

var n = 3
for n > 0 {
    Move(n, n)
    n = n - 1
}
Log("final:", transform, "| alias sees:", sentry)
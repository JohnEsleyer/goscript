// stdlib_demo.go --- GoScript v3 native standard library (math, fmt, rand, time)
// Run it with:  cargo run -- examples/stdlib_demo.go

type Vector2 struct {
    X float64
    Y float64
}

var pos = Vector2{X: 10, Y: -15}
var velocity = Vector2{X: 25, Y: 40}

func OnUpdate(dt float64) {
    // math: sqrt, abs, clamp
    var speed = math.Sqrt(velocity.X * velocity.X + velocity.Y * velocity.Y)
    pos.X = math.Clamp(pos.X + velocity.X * dt, 0, 100)

    // rand: procedural variety
    var roll = rand.Intn(100)

    // fmt: formatted UI/debug text
    var status = fmt.Sprintf("Pos X: %f | Speed: %f | Roll: %d | Abs: %f", pos.X, speed, roll, math.Abs(velocity.Y))
    fmt.Println(status)

    // time: engine frame delta
    Log("delta:", time.Delta())
}

func MaxSpeed() float64 {
    return math.Max(velocity.X, velocity.Y)
}
// enemy.gs — a simple enemy that patrols back and forth
var speed = 40.0
var dir = 1.0
var patrol_range = 80.0
var origin_x = 0.0

func OnUpdate(dt float64) {
    var pos = GetPosition()
    var x = pos[0]
    var y = pos[1]

    if origin_x == 0.0 {
        origin_x = x
    }

    MovePosition(speed * dir * dt, 0.0)

    if x > origin_x + patrol_range {
        dir = -1.0
    }
    if x < origin_x - patrol_range {
        dir = 1.0
    }

    Log(fmt.Sprintf("Enemy pos: (%.1f, %.1f)  dir=%.0f", x, y, dir))
}

// player.gs — the player entity script
var hp = 100
var speed = 120.0

func OnUpdate(dt float64) {
    var dx = GetAxis("Horizontal") * speed * dt
    var dy = GetAxis("Vertical") * speed * dt
    MovePosition(dx, dy)

    if InputKeyPressed("Space") {
        Log("Player attacks!")
    }

    var pos = GetPosition()
    Log(fmt.Sprintf("Player pos: (%.1f, %.1f)  hp=%d", pos[0], pos[1], hp))
}

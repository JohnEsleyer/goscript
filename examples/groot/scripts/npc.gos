// npc.go — Stationary NPC with event listening and color pulsing

type NPC struct {
    Dialog     string
    PulseTimer float64
}

var npc = NPC{
    Dialog:     "Hello, adventurer!",
    PulseTimer: 0.0,
}

func OnUpdate(dt float64) {
    var pos = groot.GetSelfPosition()
    var px = pos[0]
    var py = pos[1]

    // Smooth sine bob
    npc.PulseTimer = npc.PulseTimer + dt
    var bob = math.Sin(npc.PulseTimer * 3.0) * 5.0
    groot.SetSelfPosition(px, py + bob*0.01)

    // Pulsing color
    var t = (math.Sin(npc.PulseTimer * 2.0) + 1.0) / 2.0
    var alpha = groot.Lerp(0.6, 1.0, t)
    groot.SetSelfColor(0.3, 0.5, 1.0, alpha)

    // Debug circle
    groot.DrawDebugCircle(px, py, 25.0, 0.2, 0.3, 0.5)

    // Check distance to player
    var dist = groot.GetDistance(3, 1)
    if dist < 120.0 {
        groot.DrawDebugCircle(px, py, 30.0, 0.3, 0.5, 1.0)
        groot.Log(fmt.Sprintf("NPC: '%s' (player nearby! dist=%.1f)", npc.Dialog, dist))
    } else {
        groot.Log(fmt.Sprintf("NPC pos=(%.1f,%.1f) dialog=%s", px, py, npc.Dialog))
    }
}

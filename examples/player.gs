// player.gs --- a GoScript game-object demonstration
// Run it with:  cargo run -- examples/player.gs

var name = "Player"
var hp = 100
var maxHp = 100
var speed = 12.5
var position float64 = 0.0

func GetHealth() {
    return hp
}

func GetAlive() {
    return hp > 0
}

func TakeDamage(amount int) {
    if amount < 0 {
        return
    }
    hp = hp - amount
    if hp <= 0 {
        hp = 0
        Log("entity defeated!")
    } else {
        Log("took", amount, "damage, HP is now", hp)
    }
}

func Heal(amount int) {
    hp = hp + amount
    if hp > maxHp {
        hp = maxHp
    }
    Log("healed to", hp)
}

func Add(a int, b int) {
    return a + b
}

func RestartLevel() {
    hp = maxHp
    position = 0.0
    Log("level restarted, HP =", hp)
}

func OnUpdate(dt float64) {
    if hp > 0 {
        position = position + speed*dt
        TakeDamage(10)
        Heal(4)
        Log("position:", position)
        Log("2 + 3 =", Add(2, 3))
    } else {
        RestartLevel()
    }
}

OnUpdate(0.016)
OnUpdate(0.016)
Log("final HP:", hp, "alive:", GetAlive(), "position:", position)
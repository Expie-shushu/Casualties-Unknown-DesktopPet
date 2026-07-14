# configs/

放置桌宠的配置文件。

| 文件 | 说明 |
|------|------|
| `chatter.json` | 气泡对话语录，按场景分池 |

### chatter.json 结构

```json
{
  "version": 1,
  "lines": [...],
  "eatLines": [...],
  "drinkLines": [...],
  ...
}
```

| 字段 | 触发场景 |
|------|---------|
| `lines` | 空闲随机闲聊 |
| `eatLines` | 进食后 |
| `drinkLines` | 喝水后 |
| `gameStartLines` | 猜拳开局 |
| `wheelStartLines` | 转盘开局 |
| `greetingLines` | 双击打招呼 |
| `gameWinLines` | 猜拳胜利 |
| `gameDrawLines` | 猜拳平局 |
| `gameLoseLines` | 猜拳失败 |
| `inventoryFullLines` | 物品栏已满 |
| `equipLines` | 装备物品 |
| `musicOnLines` | 音乐开启 |
| `musicOffLines` | 音乐关闭 |
| `pushupLines` | 训练—俯卧撑 |
| `squatLines` | 训练—深蹲 |
| `plankLines` | 训练—平板支撑 |
| `trainingStageLines` | 训练完整一轮结束 |
| `trainingRewardLines` | 训练成果结算 |
| `needsLines` | 需求状态相关（心情/饥饿/口渴） |


# Casualties Unknown DesktopPet

> 本项目是基于游戏 Casualties Unknown（原 Scav Prototype）实现的 Windows 桌面互动宠物，支持骨骼动画以及与用户的互动。
>
> ——*若灰星的他无法拯救，那就请珍惜屏幕前的他*

## 技术栈

- **Rust** — 核心语言
- **wgpu + winit** — 图形渲染 & 窗口管理
- **egui** — 设置面板 GUI
- **cosmic-text** — 文字渲染
- **rodio** — 音频播放
- **muda + tray-icon** — 右键菜单 & 系统托盘

## 功能

- **骨骼动画** — 多部位组装，动画状态机驱动，支持动作混合与平滑过渡
- **养成数值** — 心情、饥饿、口渴实时衰减，影响宠物表情与行为反馈
- **物品栏与投喂** — 多槽位物品栏，支持背包扩展，拖拽食物直接喂养
- **音乐播放器** — 支持多种音频格式，顺序、列表循环、单曲循环三种模式
- **猜拳游戏** — 赢取硬币兑换奖励
- **奖励转盘** — 食品、饮品、背包随机抽取
- **表情meme** — 根据状态自动弹出，支持静态图PNG、JPG与 GIF 动图
- **气泡对话** — 场景感知的随机闲聊气泡
- **系统托盘** — 最小化到托盘，窗口穿透，全屏自动隐藏

## 项目结构

```
├── src/                     # Rust 源码
│   ├── main.rs              # 程序入口
│   ├── app.rs               # 核心应用状态 & 主循环
│   ├── animator.rs          # 骨骼动画系统
│   ├── needs.rs             # 养成数值（心情/饥饿/口渴）
│   ├── inventory.rs         # 物品栏逻辑
│   ├── music.rs             # 音乐播放
│   ├── interact.rs          # 多宠互动
│   ├── sticker.rs           # 表情贴纸
│   ├── rpsGame.rs           # 猜拳游戏
│   ├── rewardWheel.rs       # 奖励转盘
│   ├── plugin.rs            # JS 插件主机
│   └── ...
├── desktopPet/              # 资源文件
│   ├── anim/                # 动画控制器 & 片段
│   ├── configs/             # 对话配置
│   ├── foods/               # 食物图标
│   ├── stickers/            # 表情贴纸
│   ├── music/               # 音乐文件
│   ├── fonts/               # 字体文件
│   ├── plugins/             # JS 插件
│   ├── poses/               # 骨骼姿态
│   └── ...
├── data/                    # 皮肤 & 配件
│   ├── skin/                # 多套皮肤素材
│   └── accessories.json     # 配件定义
├── icons/                   # 应用图标
├── build.rs                 # 构建脚本
└── Cargo.toml               # 项目配置
```

## 快速开始

### 前置要求

- Rust 1.85+
- Windows 10/11

### 编译运行

```
cargo build --release
```

编译完成后可通过以下任一方式均可启动：

- 点击 `target/release/Casualties_Unknown_desktopPet.exe`
- 命令行启动：`./target/release/Casualties_Unknown_desktopPet.exe`
- 使用桌面快捷方式 `CU DesktopPet.lnk`（Release 构建时自动生成）

## 碎碎念 
- 该项目最初只是灵光一闪闹着玩，没想到真下手去实践的时候发现困难重重，不过项目整体还是实现好了
- 目前低心情的动作逻辑存在问题，骨骼动画播放不全，比较抽象。但你都让你的exp心情低落了，~~就不要来找我的问题了~~，如果后续有时间的话会尝试修改的（~~这不是画大饼~~）
- 项目里很多素材是由gpt所画，如果你想看作者自己画从desktop/wheel下就可以窥见一二，然后发现还是Ai画的顺眼
- 这是作者第一次创建并发布项目，若其中有些内容书写或MIT LICENSE等内容有问题或不符合社区规范，请告诉作者，~~作者会第一个跑~~作者会修改的

## 致谢

- 角色形象与动画素材来源于游戏 [Casualties Unknown（原 Scav Prototype）](https://orsonik.itch.io/)，作者 [Orsoniks](https://github.com/Orsoniks)
- 动作模组提取与基础框架作者：[huanxin996](https://github.com/huanxin996)
- 功能开发与扩展作者：[Expie鼠鼠](https://github.com/Expie-shushu)

## 许可证

本项目基于 MIT 协议开源。

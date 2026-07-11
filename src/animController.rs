// Unity AnimatorController JSON 加载：与 tools/anim/exportController.py 输出格式对齐的纯数据结构。
#![allow(non_snake_case)]

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AnimController {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub clips: Vec<ClipRef>,
    pub layers: Vec<Layer>,
    pub stateMachines: Vec<StateMachine>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Parameter {
    pub id: u32,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub paramType: String,
    pub valueIndex: i32,
    #[serde(default)]
    pub defaultBool: Option<bool>,
    #[serde(default)]
    pub defaultFloat: Option<f32>,
    #[serde(default)]
    pub defaultInt: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipRef {
    pub index: usize,
    pub name: String,
    pub pathId: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    pub binding: u32,
    pub stateMachineIndex: usize,
    pub defaultWeight: f32,
    pub blendingMode: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StateMachine {
    pub defaultStateIndex: usize,
    pub states: Vec<State>,
    #[serde(default)]
    pub selectorStates: Vec<SelectorState>,
    #[serde(default)]
    pub anyStateTransitions: Vec<Transition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct State {
    pub id: u32,
    pub nameId: u32,
    pub name: Option<String>,
    pub fullPathId: u32,
    pub fullPath: Option<String>,
    pub speed: f32,
    pub speedParamId: u32,
    pub cycleOffset: f32,
    pub mirror: bool,
    #[serde(rename = "loop", default)]
    pub looping: bool,
    pub timeParamId: u32,
    pub blendTreeConstantIndexArray: Vec<i32>,
    #[serde(default)]
    pub blendTrees: Vec<BlendTreeConstant>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub leafInfoArray: Vec<LeafInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectorState {
    pub fullPathId: u32,
    pub fullPath: Option<String>,
    pub isEntry: bool,
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LeafInfo {
    #[serde(default)]
    pub indexOffset: i32,
    #[serde(default)]
    pub idArray: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlendTreeConstant {
    pub nodes: Vec<BlendTreeNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlendTreeNode {
    pub blendType: u32,
    pub blendEventId: u32,
    pub blendEventName: Option<String>,
    pub blendEventYId: u32,
    pub blendEventYName: Option<String>,
    pub children: Vec<BlendTreeChild>,
    pub clipIndex: i32,
    pub duration: f32,
    pub isLeaf: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlendTreeChild {
    pub nodeIndex: usize,
    #[serde(default)]
    pub threshold: f32,
    #[serde(default)]
    pub positionX: f32,
    #[serde(default)]
    pub positionY: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transition {
    pub destStateIndex: i64,
    pub fullPathId: u32,
    pub id: u32,
    pub transitionDuration: f32,
    pub transitionOffset: f32,
    pub exitTime: f32,
    pub hasFixedDuration: bool,
    pub hasExitTime: bool,
    pub canTransitionToSelf: bool,
    pub atomic: bool,
    pub isExit: bool,
    pub conditions: Vec<TransitionCondition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionCondition {
    pub paramId: u32,
    pub paramName: Option<String>,
    pub mode: u32,
    pub modeName: Option<String>,
    pub threshold: f32,
    pub exitTime: f32,
}

pub fn loadController(path: &Path) -> Result<AnimController> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read controller {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parse controller {}", path.display()))
}

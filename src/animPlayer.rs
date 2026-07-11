// Mecanim 状态机播放器：参数喂入 → transition 评估 → BlendTree 求值 → clip 加权插值 → crossfade。
#![allow(non_snake_case)]

use std::collections::HashMap;

use crate::animClip::{evaluateClip, loopOrClampTime, AnimClip, LimbSample};
use crate::animController::{
    AnimController, BlendTreeConstant, BlendTreeNode, State, Transition, TransitionCondition,
};

#[derive(Debug, Clone, Copy)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Trigger(bool),
}

impl ParamValue {
    pub fn asFloat(&self) -> f32 {
        match self {
            ParamValue::Float(v) => *v,
            ParamValue::Int(v) => *v as f32,
            ParamValue::Bool(v) | ParamValue::Trigger(v) => if *v { 1.0 } else { 0.0 },
        }
    }
    pub fn asBool(&self) -> bool {
        match self {
            ParamValue::Bool(v) | ParamValue::Trigger(v) => *v,
            ParamValue::Float(v) => *v != 0.0,
            ParamValue::Int(v) => *v != 0,
        }
    }
}

pub struct AnimPlayer {
    pub controller: AnimController,
    pub clipsByName: HashMap<String, AnimClip>,
    paramById: HashMap<u32, ParamValue>,
    nameToId: HashMap<String, u32>,
    layerStateIdx: Vec<usize>,
    layerStateTime: Vec<f32>,
    layerCrossfade: Vec<Option<Crossfade>>,
}

#[derive(Debug, Clone, Copy)]
struct Crossfade {
    fromStateIdx: usize,
    fromStateTime: f32,
    duration: f32,
    elapsed: f32,
}

impl AnimPlayer {
    pub fn new(controller: AnimController, clipsByName: HashMap<String, AnimClip>) -> Self {
        let mut paramById = HashMap::new();
        let mut nameToId = HashMap::new();
        for p in &controller.parameters {
            let v = match p.paramType.as_str() {
                "bool" => ParamValue::Bool(p.defaultBool.unwrap_or(false)),
                "trigger" => ParamValue::Trigger(p.defaultBool.unwrap_or(false)),
                "int" => ParamValue::Int(p.defaultInt.unwrap_or(0)),
                _ => ParamValue::Float(p.defaultFloat.unwrap_or(0.0)),
            };
            paramById.insert(p.id, v);
            if let Some(name) = &p.name {
                nameToId.insert(name.clone(), p.id);
            }
        }
        let layerCount = controller.layers.len();
        let mut layerStateIdx = Vec::with_capacity(layerCount);
        for layer in &controller.layers {
            let smIdx = layer.stateMachineIndex;
            let sm = &controller.stateMachines[smIdx];
            layerStateIdx.push(sm.defaultStateIndex);
        }
        let layerStateTime = vec![0.0; layerCount];
        let layerCrossfade = vec![None; layerCount];
        Self {
            controller,
            clipsByName,
            paramById,
            nameToId,
            layerStateIdx,
            layerStateTime,
            layerCrossfade,
        }
    }

    pub fn setFloat(&mut self, name: &str, value: f32) {
        if let Some(id) = self.nameToId.get(name) {
            self.paramById.insert(*id, ParamValue::Float(value));
        }
    }
    pub fn setInt(&mut self, name: &str, value: i32) {
        if let Some(id) = self.nameToId.get(name) {
            self.paramById.insert(*id, ParamValue::Int(value));
        }
    }
    pub fn setBool(&mut self, name: &str, value: bool) {
        if let Some(id) = self.nameToId.get(name) {
            self.paramById.insert(*id, ParamValue::Bool(value));
        }
    }

    pub fn currentStateName(&self, layer: usize) -> Option<&str> {
        let sm = &self.controller.stateMachines[self.controller.layers[layer].stateMachineIndex];
        let s = &sm.states[self.layerStateIdx[layer]];
        s.name.as_deref().or(s.fullPath.as_deref())
    }

    pub fn playState(&mut self, layer: usize, stateName: &str) -> bool {
        if layer >= self.controller.layers.len() {
            return false;
        }
        let smIdx = self.controller.layers[layer].stateMachineIndex;
        let sm = &self.controller.stateMachines[smIdx];
        for (i, s) in sm.states.iter().enumerate() {
            if s.name.as_deref() == Some(stateName) {
                self.layerStateIdx[layer] = i;
                self.layerStateTime[layer] = 0.0;
                self.layerCrossfade[layer] = None;
                return true;
            }
        }
        false
    }

    pub fn update(&mut self, dt: f32) {
        let layerCount = self.controller.layers.len();
        for li in 0..layerCount {
            self.tickLayer(li, dt);
        }
    }

    fn tickLayer(&mut self, layer: usize, dt: f32) {
        let smIdx = self.controller.layers[layer].stateMachineIndex;
        let stateCount = self.controller.stateMachines[smIdx].states.len();
        if stateCount == 0 {
            return;
        }
        let curIdx = self.layerStateIdx[layer];
        let curState = self.controller.stateMachines[smIdx].states[curIdx].clone();
        let curStateDuration = self.stateDuration(&curState).max(1e-3);
        let normalizedTime = self.layerStateTime[layer] / curStateDuration;
        let speed = self.resolveStateSpeed(&curState);
        self.layerStateTime[layer] += dt * speed;

        if let Some(cf) = self.layerCrossfade[layer].as_mut() {
            cf.elapsed += dt;
            if cf.elapsed >= cf.duration {
                self.layerCrossfade[layer] = None;
            }
        }

        let anyTrans = self.controller.stateMachines[smIdx].anyStateTransitions.clone();
        if let Some(t) = self.firstSatisfiedTransition(&anyTrans, normalizedTime, true) {
            self.applyTransition(layer, &t);
            return;
        }
        let curTrans = curState.transitions.clone();
        if let Some(t) = self.firstSatisfiedTransition(&curTrans, normalizedTime, false) {
            self.applyTransition(layer, &t);
        }
    }

    fn applyTransition(&mut self, layer: usize, t: &Transition) {
        let smIdx = self.controller.layers[layer].stateMachineIndex;
        if t.isExit {
            return;
        }
        let dest = t.destStateIndex;
        if dest < 0 {
            return;
        }
        let destIdx = dest as usize;
        if destIdx >= self.controller.stateMachines[smIdx].states.len() {
            return;
        }
        let fromIdx = self.layerStateIdx[layer];
        let fromTime = self.layerStateTime[layer];
        self.layerStateIdx[layer] = destIdx;
        let destState = &self.controller.stateMachines[smIdx].states[destIdx];
        let destDur = self.stateDuration(destState).max(1e-3);
        self.layerStateTime[layer] = t.transitionOffset * destDur;
        if t.transitionDuration > 1e-4 {
            self.layerCrossfade[layer] = Some(Crossfade {
                fromStateIdx: fromIdx,
                fromStateTime: fromTime,
                duration: t.transitionDuration,
                elapsed: 0.0,
            });
        } else {
            self.layerCrossfade[layer] = None;
        }
    }

    fn firstSatisfiedTransition(
        &self,
        transitions: &[Transition],
        normalizedTime: f32,
        isAnyState: bool,
    ) -> Option<Transition> {
        for t in transitions {
            if t.hasExitTime && !isAnyState && normalizedTime < t.exitTime {
                continue;
            }
            let mut allOk = true;
            for cond in &t.conditions {
                if !self.evaluateCondition(cond) {
                    allOk = false;
                    break;
                }
            }
            if allOk {
                return Some(t.clone());
            }
        }
        None
    }

    fn evaluateCondition(&self, c: &TransitionCondition) -> bool {
        let v = match self.paramById.get(&c.paramId) {
            Some(v) => *v,
            None => return false,
        };
        match c.mode {
            1 => v.asBool(),
            2 => !v.asBool(),
            3 => v.asFloat() > c.threshold,
            4 => v.asFloat() < c.threshold,
            5 => (v.asFloat() - c.threshold).abs() < 0.5,
            6 => (v.asFloat() - c.threshold).abs() >= 0.5,
            _ => false,
        }
    }

    fn resolveStateSpeed(&self, state: &State) -> f32 {
        if state.speedParamId != 0 {
            if let Some(v) = self.paramById.get(&state.speedParamId) {
                return v.asFloat();
            }
        }
        state.speed
    }

    fn stateDuration(&self, state: &State) -> f32 {
        if let Some(bt) = state.blendTrees.first() {
            if let Some(root) = bt.nodes.first() {
                if root.duration > 0.0 {
                    return root.duration;
                }
            }
        }
        let blends = self.evaluateState(state);
        let mut maxDur: f32 = 0.0;
        for (clipName, _) in &blends {
            if let Some(c) = self.clipsByName.get(clipName) {
                if c.duration > maxDur {
                    maxDur = c.duration;
                }
            }
        }
        if maxDur < 1e-3 { 1.0 } else { maxDur }
    }

    fn evaluateState(&self, state: &State) -> Vec<(String, f32)> {
        if let Some(bt) = state.blendTrees.first() {
            return self.evaluateBlendTree(bt);
        }
        Vec::new()
    }

    fn evaluateBlendTree(&self, bt: &BlendTreeConstant) -> Vec<(String, f32)> {
        if bt.nodes.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.collectBlendChildren(bt, 0, 1.0, &mut out);
        out
    }

    fn collectBlendChildren(
        &self,
        bt: &BlendTreeConstant,
        nodeIdx: usize,
        weight: f32,
        out: &mut Vec<(String, f32)>,
    ) {
        if nodeIdx >= bt.nodes.len() {
            return;
        }
        let node = &bt.nodes[nodeIdx];
        if node.isLeaf {
            self.pushClipForLeaf(node, weight, out);
            return;
        }
        if node.children.is_empty() {
            return;
        }
        match node.blendType {
            0 => self.evaluate1D(bt, node, weight, out),
            1 | 2 | 3 => self.evaluate2D(bt, node, weight, out),
            _ => {
                self.collectBlendChildren(bt, node.children[0].nodeIndex, weight, out);
            }
        }
    }

    fn evaluate2D(
        &self,
        bt: &BlendTreeConstant,
        node: &BlendTreeNode,
        weight: f32,
        out: &mut Vec<(String, f32)>,
    ) {
        let px = self.paramById.get(&node.blendEventId).map(|p| p.asFloat()).unwrap_or(0.0);
        let py = self.paramById.get(&node.blendEventYId).map(|p| p.asFloat()).unwrap_or(0.0);
        let weights: Vec<(usize, f32)> = node
            .children
            .iter()
            .map(|c| {
                let dx = px - c.positionX;
                let dy = py - c.positionY;
                let dsq = dx * dx + dy * dy;
                let w = 1.0 / (dsq + 1e-3).powi(2);
                (c.nodeIndex, w)
            })
            .collect();
        let total: f32 = weights.iter().map(|(_, w)| *w).sum();
        if total <= 0.0 {
            self.collectBlendChildren(bt, node.children[0].nodeIndex, weight, out);
            return;
        }
        for (idx, w) in &weights {
            let normalized = w / total;
            if normalized > 0.001 {
                self.collectBlendChildren(bt, *idx, weight * normalized, out);
            }
        }
    }

    fn evaluate1D(
        &self,
        bt: &BlendTreeConstant,
        node: &BlendTreeNode,
        weight: f32,
        out: &mut Vec<(String, f32)>,
    ) {
        let v = self
            .paramById
            .get(&node.blendEventId)
            .map(|p| p.asFloat())
            .unwrap_or(0.0);
        let mut sorted: Vec<(usize, f32)> = node
            .children
            .iter()
            .map(|c| (c.nodeIndex, c.threshold))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if sorted.is_empty() {
            return;
        }
        if v <= sorted[0].1 {
            self.collectBlendChildren(bt, sorted[0].0, weight, out);
            return;
        }
        let last = sorted.len() - 1;
        if v >= sorted[last].1 {
            self.collectBlendChildren(bt, sorted[last].0, weight, out);
            return;
        }
        for i in 0..last {
            let (loIdx, loT) = sorted[i];
            let (hiIdx, hiT) = sorted[i + 1];
            if v >= loT && v <= hiT {
                let span = (hiT - loT).max(1e-5);
                let w = (v - loT) / span;
                self.collectBlendChildren(bt, loIdx, weight * (1.0 - w), out);
                self.collectBlendChildren(bt, hiIdx, weight * w, out);
                return;
            }
        }
    }

    fn pushClipForLeaf(&self, node: &BlendTreeNode, weight: f32, out: &mut Vec<(String, f32)>) {
        if node.clipIndex < 0 {
            return;
        }
        let idx = node.clipIndex as usize;
        if idx >= self.controller.clips.len() {
            return;
        }
        let name = &self.controller.clips[idx].name;
        out.push((name.clone(), weight));
    }

    /// 取当前所有 layer 的合成 limb 位姿；每帧渲染前调一次。
    pub fn evaluate(&self) -> HashMap<String, LimbSample> {
        let mut acc: HashMap<String, LimbSample> = HashMap::new();
        let mut accWeight: HashMap<String, f32> = HashMap::new();
        for layer in 0..self.controller.layers.len() {
            self.evaluateLayerInto(layer, &mut acc, &mut accWeight);
        }
        for (k, sample) in acc.iter_mut() {
            let w = accWeight.get(k).copied().unwrap_or(0.0);
            if w > 1e-4 {
                normalizeSample(sample, w);
            }
        }
        acc
    }

    fn evaluateLayerInto(
        &self,
        layer: usize,
        acc: &mut HashMap<String, LimbSample>,
        accWeight: &mut HashMap<String, f32>,
    ) {
        let smIdx = self.controller.layers[layer].stateMachineIndex;
        let curIdx = self.layerStateIdx[layer];
        let sm = &self.controller.stateMachines[smIdx];
        let curState = &sm.states[curIdx];
        let curBlends = self.evaluateState(curState);
        let curDur = self.stateDuration(curState).max(1e-3);
        let curT = if isLoopingState(curState) {
            (self.layerStateTime[layer] / curDur).rem_euclid(1.0)
        } else {
            (self.layerStateTime[layer] / curDur).clamp(0.0, 1.0)
        };

        let (toW, fromW) = match self.layerCrossfade[layer] {
            Some(cf) if cf.duration > 1e-4 => {
                let progress = (cf.elapsed / cf.duration).clamp(0.0, 1.0);
                (progress, 1.0 - progress)
            }
            _ => (1.0, 0.0),
        };

        let layerWeight = self.controller.layers[layer].defaultWeight.max(0.0);
        let layerWeight = if layer == 0 { 1.0 } else { layerWeight };

        accumulateBlends(
            &self.clipsByName,
            &curBlends,
            curT,
            isLoopingState(curState),
            layerWeight * toW,
            acc,
            accWeight,
        );
        if let Some(cf) = self.layerCrossfade[layer] {
            if fromW > 1e-4 {
                let fromState = &sm.states[cf.fromStateIdx];
                let fromBlends = self.evaluateState(fromState);
                let fromDur = self.stateDuration(fromState).max(1e-3);
                let fromT = if isLoopingState(fromState) {
                    (cf.fromStateTime / fromDur).rem_euclid(1.0)
                } else {
                    (cf.fromStateTime / fromDur).clamp(0.0, 1.0)
                };
                accumulateBlends(
                    &self.clipsByName,
                    &fromBlends,
                    fromT,
                    isLoopingState(fromState),
                    layerWeight * fromW,
                    acc,
                    accWeight,
                );
            }
        }
    }
}

fn isLoopingState(state: &State) -> bool {
    state.looping || state.blendTrees.first().map_or(false, |bt| bt.nodes.len() > 1)
}

fn accumulateBlends(
    clipsByName: &HashMap<String, AnimClip>,
    blends: &[(String, f32)],
    normalizedTime: f32,
    looping: bool,
    layerWeight: f32,
    acc: &mut HashMap<String, LimbSample>,
    accWeight: &mut HashMap<String, f32>,
) {
    for (clipName, weight) in blends {
        let clip = match clipsByName.get(clipName) {
            Some(c) => c,
            None => continue,
        };
        let clipTime = loopOrClampTime(clip, normalizedTime * clip.duration, looping);
        let samples = evaluateClip(clip, clipTime);
        let w = weight * layerWeight;
        if w < 1e-5 {
            continue;
        }
        for (limbName, s) in samples {
            let entry = acc.entry(limbName.clone()).or_insert_with(LimbSample::defaultRest);
            mergeSample(entry, &s, w);
            *accWeight.entry(limbName).or_insert(0.0) += w;
        }
    }
}

fn mergeSample(entry: &mut LimbSample, s: &LimbSample, w: f32) {
    entry.px += s.px * w;
    entry.py += s.py * w;
    entry.pz += s.pz * w;
    entry.eulerX += s.eulerX * w;
    entry.eulerY += s.eulerY * w;
    entry.eulerZ += s.eulerZ * w;
    entry.scaleX += s.scaleX * w;
    entry.scaleY += s.scaleY * w;
    entry.scaleZ += s.scaleZ * w;
    entry.hasPos = entry.hasPos || s.hasPos;
    entry.hasEuler = entry.hasEuler || s.hasEuler;
}

fn normalizeSample(entry: &mut LimbSample, totalWeight: f32) {
    let inv = 1.0 / totalWeight;
    entry.px *= inv;
    entry.py *= inv;
    entry.pz *= inv;
    entry.eulerX *= inv;
    entry.eulerY *= inv;
    entry.eulerZ *= inv;
    entry.scaleX *= inv;
    entry.scaleY *= inv;
    entry.scaleZ *= inv;
}

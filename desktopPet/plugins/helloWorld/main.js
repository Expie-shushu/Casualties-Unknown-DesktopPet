// 桌宠示例插件。注册 wave 动作 + 双击事件触发。

function setup(api) {
  api.log.info("loaded");

  api.registerMotion("wave", function (t) {
    var out = new Map();
    var s = Math.sin(2 * Math.PI * 1.4 * t);
    out.set("UpArmF", { dRotZ: -90 + s * 12 });
    out.set("DownArmF", { dRotZ: 30 + s * 18 });
    out.set("Head", { dRotZ: s * 4 });
    return out;
  });

  function onDbl() {
    api.pet.say("hello from plugin", 1500);
    api.pet.playMotion("wave", 1800);
  }
  api.on("dblclick", onDbl);

  return function dispose() {
    api.off("dblclick", onDbl);
    api.log.info("unloaded");
  };
}

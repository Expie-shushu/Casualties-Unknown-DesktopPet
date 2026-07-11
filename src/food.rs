// 食物定义表。id 即素材文件名 desktopPet/foods/<id>.png。
#![allow(non_snake_case)]

/// 食物分类：食品（主补饥饿，"吃"）与饮品（主补口渴，"喝"）。
/// 投喂时据此选用 chatter.json 的 eatLines / drinkLines 台词池。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoodKind {
    /// 食品（吃）。
    Eat,
    /// 饮品（喝）。
    Drink,
}

#[derive(Clone, Copy, Debug)]
pub struct FoodDef {
    /// 食物品类数目
    pub number_id: i16,
    /// 文件名 / 库存键。
    pub id: &'static str,
    /// 仓库显示名（中文）。
    pub nameZh: &'static str,
    /// 食品 / 饮品类型。
    pub kind: FoodKind,
    pub hunger: f32,
    pub thirst: f32,
    pub mood: f32,
}

/// 食物定义表。id 对应 desktopPet/foods/<id>.png。
pub static FOODS: &[FoodDef] = &[
    FoodDef { number_id: 1,       id: "apple",                   nameZh: "苹果",   kind: FoodKind::Eat,   hunger: 10.0, thirst: 7.0, mood: 5.0 },
    FoodDef { number_id: 2,       id: "bread",                   nameZh: "面包",   kind: FoodKind::Eat,   hunger: 20.0, thirst: -5.0, mood: 3.0 },
    FoodDef { number_id: 3,       id: "dog food",                nameZh: "狗粮",   kind: FoodKind::Eat,   hunger: 30.0, thirst: -3.0, mood: 2.0 },
    FoodDef { number_id: 4,       id: "energy bar",              nameZh: "能量棒", kind: FoodKind::Eat,   hunger: 40.0, thirst: -5.0, mood: 8.0 },
    FoodDef { number_id: 5,       id: "fried chicken",           nameZh: "炸鸡",   kind: FoodKind::Eat,   hunger: 45.0, thirst: -7.0, mood: 15.0 },
    FoodDef { number_id: 6,       id: "water",                   nameZh: "水",     kind: FoodKind::Drink, hunger: 0.0,  thirst: 25.0, mood: 3.0 },
    FoodDef { number_id: 7,       id: "apple juice",             nameZh: "苹果汁", kind: FoodKind::Drink, hunger: 5.0,  thirst: 20.0, mood: 4.0 },
    FoodDef { number_id: 8,       id: "cherry",                  nameZh: "樱桃",   kind: FoodKind::Eat,   hunger: 15.0, thirst: 10.0, mood: 5.0 },
    FoodDef { number_id: 9,       id: "The piece of watermelon", nameZh: "西瓜片", kind: FoodKind::Eat,   hunger: 20.0, thirst: 10.0, mood: 10.0 },
    FoodDef { number_id: 10,      id: "milk",                    nameZh: "牛奶",   kind: FoodKind::Drink, hunger: 7.0,  thirst: 15.0, mood: 8.0 },
    FoodDef { number_id: 11,      id: "peach",                   nameZh: "桃子~",  kind: FoodKind::Eat,   hunger: 12.0, thirst: 12.0, mood: 3.0 },
    FoodDef { number_id: 12,      id: "banana",                  nameZh: "香蕉",   kind: FoodKind::Eat,   hunger: -1.0 , thirst: 3.5, mood: 2.0 }
];

/// 按 id 查询食物定义。
pub fn foodById(id: &str) -> Option<&'static FoodDef> {
    FOODS.iter().find(|f| f.id == id)
}


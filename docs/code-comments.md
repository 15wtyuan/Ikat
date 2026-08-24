# 注释哲学

> 本文摆的是哲学、出处与范例，不是操作步骤。读完之后如何落笔，是读者自己的判断——本文相信判断力，只提供判断的养料。
> 引文均经核实（书籍原文 / 官方规范 / 项目源码），讹传变体不用。

## 一、两种对立的回答

关于「注释是什么」，这个行业的两位最具代表性的作者给出过几乎相反的回答，并在 2024–2025 年进行了一场逐轮书面辩论（johnousterhout/aposd-vs-clean-code）。

**Robert C. Martin（《Clean Code》第 4 章）**：注释是失败的补偿——

> "The proper use of comments is to compensate for our failure to express ourselves in code… Comments are always failures."
> （注释的正当用途是补偿我们未能用代码表达自己……注释总是失败。）
>
> "Every time you write a comment, you should grimace and feel the failure of your ability of expression."
> （每次写注释你都该做个鬼脸，为自己表达能力的失败感到痛心。）

在 Martin 的世界观里，真相只存在于代码中（"Truth can only be found in one place: the code"），好命名和小函数是第一公民，注释是二等的、防御性的、最后手段。他并非一刀切——决策意图、后果警告、公共 API 文档他承认必须写——但整体姿态是：每次写注释前先问，怎样改代码能让这条注释不再需要。

**John Ousterhout（《A Philosophy of Software Design》）**：注释是抽象的根本组件——

> "Comments are fundamental to abstractions."
>
> "The overall idea behind comments is to capture information that was in the mind of the designer but couldn't be represented in the code."
> （注释的总思路：捕获设计者脑中、但代码呈现不了的信息。）

他称「好代码自文档化」为 "a delicious myth"；主张**先写注释再写代码**（"Write the comments first"），让注释成为设计工具——写不出简洁的接口注释，是抽象没想清楚的信号，与 TDD 中「写不出测试说明设计有问题」同构。他在辩论中自述：同样的代码他会写 Martin 5–10 倍量的注释；而「缺注释的代价轻松达到坏注释代价的 10–100 倍」。但他同样反对函数体内的注释泛滥："Most methods I write have no comments in the body, just a header comment describing the interface."

这场辩论唯一被双方明确认可的共识是：

> "Implementation code only needs comments when the code is nonobvious."
> （实现代码只在非显而易见时需要注释。）

这句话几乎就是整个行业最大公约数。分歧从来不在「该不该写」，而在**量级、密度与姿态**。

## 二、对立之下，共识的地面

两极之下，有一层几乎所有经典来源共同承认的公理：

**复述代码的注释是噪音。** Kernighan & Plauger 在 1974 年《The Elements of Programming Style》里就写下了那条被引用了五十年的规则——

> "Don't comment bad code—rewrite it."
> （不要给烂代码写注释——重写它。）

（顺带考证：这句常被误归给 Martin，实为 Kernighan & Plauger 原文；Martin 在《Clean Code》第 4 章卷首引用了它，还把 Plauger 拼错了。）

**注释补偿不了坏命名。** 《The Art of Readable Code》（Boswell & Foucher）：

> "A good name is better than a good comment because it will be seen everywhere the function is used."
> （好名字胜过好注释，因为函数用到的地方都看得到名字，注释只在定义处可见。）

**DRY 管的是知识，不是文本。** 《The Pragmatic Programmer》的 DRY 常被误读为「注释违反 DRY」。它的原文是 "Every piece of knowledge must have a single, unambiguous, authoritative representation within a system"——复述函数行为的注释与代码构成同一知识的双重表示，会漂移，是违规；而记录代码中根本不存在的知识（为什么、约束、出处），本来就是唯一表示，从不违规。

**注释的目的读者是下一个读代码的人。** Google C++ 风格指南："Be generous — the next one may be you!"。antirez（Redis 作者）说得更直白：代码是写给其他人读的，不只是给机器执行的。

**过期注释比没有注释更糟。** 这是 Martin 对行业的最大贡献之一——注释带着「曾经为真」的权威性主动误导。Kernighan & Pike《The Practice of Programming》将其列为五原则之一："Don't contradict the code."

**注释是写作者的认知快照。** 《The Art of Readable Code》开篇："The purpose of commenting is to help the reader know as much as the writer did."（让读者知道得和作者一样多）——包括为什么走这条路、否决了什么、哪里出乎意料。

## 三、实践的谱系

大型开源项目在这条光谱上各有其位，从最保守到最密集：

**Git（最保守一极）**。CodingGuidelines 明文承认注释的宿命：

> "Comments invariably tend to stale out when the code they were describing changes. Often splitting a function into two makes the intention of the code much clearer."
> （代码变更后注释必然腐化。把一个函数拆成两个，往往比注释更能让意图清晰。）

Git 的选择是信任结构胜过信任注释——API 文档例外，必须在头文件里、以 strbuf.h 为范本。

**Linux 内核（中间偏严）**。官方 coding-style 的措辞值得照抄原文，因为流传中常被记错——不是 "why not what"，而是：

> "Generally, you want your comments to tell WHAT your code does, not HOW."
> "NEVER try to explain HOW your code works in a comment… it's a waste of time to explain badly written code."
> "Try to avoid adding comments inside a function body: comment the function, telling people what it does, and possibly WHY it does it."

函数头承担 what 与 why；函数体尽量干净，例外是标记「特别聪明或丑陋」之处。实际代码是最好的注脚——`mm/oom_kill.c` 的文件头直接宣告本文件兼任新手教程；`out_of_memory()` 前的决策注释是 why 注释的教科书：

```c
/*
 * If we run out of memory, we have the choice between either
 * killing a random task (bad), letting the system crash (worse)
 * OR try to be smart about which process to kill. Note that we
 * don't have to be perfect here, we just have to be good.
 */
```

**Google C++ 风格指南（中间偏严，分层是其精髓）**：

> "Declaration comments describe use of the function; comments at the definition of a function describe operation."
>
> "If there is anything tricky about how a function does its job… explain why you chose to implement the function in the way you did rather than using a viable alternative."

声明处讲「怎么用」（参数语义、所有权、单位、null 含义），定义处讲「为何这么实现、弃了什么替代方案」。几乎每个函数声明都要注释（私有方法不豁免），但简单 getter、多数 override、trivial 析构明确豁免——强制与豁免都写在明处。

**SQLite（密集一极）**。它的解法是让文档住进注释：API 文档由脚本从源码注释中抽取生成，"Keeping the official documentation (in comments) and the source code close together helps ensure that they are in agreement."（文档与代码物理同址，就不会漂移）。合并文件约 40% 的行是空行与注释，每个文件头是日期、声明与职责段落。

**antirez / Redis（密集一极，方法论最成体系）**。他的博客《code comments》给出完整的分类法：

- **Why 注释**：解释原因，哪怕代码做什么已一清二楚。
- **函数注释**："The goal of a function comment is to prevent the reader from reading code in the first place."（目标是让读者根本不必读代码）——留在代码里的内联 API 文档，文档与代码精确同步。
- **设计注释**：算法与结构选择的高层综述，读实现前先读它。
- **Guide 注释**：长函数体内的分段导航——「大多数人认为最没用的注释类型」，他认为恰恰是降低认知负荷的利器。

> "Comments are rubber duck debugging on steroids."
> （注释是加强版小黄鸭调试——向未来读者解释行为时，经常自己发现 bug。）

反面同样三类：trivial 注释（读注释的认知负荷不低于读代码就不该写）、backup 注释（注释掉的旧代码——"source code is not for making backups"，源码不是备份工具）、debt 注释（TODO，尽量少但好过遗忘问题）。

**Rust（谱系的尽头：把注释二分做进语言）**。《The Rust Reference》里，普通 `//` 被定义为 "a form of whitespace"——对编译器无语义，纯粹给人读；而 `///` 不是注释，是 `#[doc="..."]` 属性，进 rustdoc 管线、默认作为 doctest 编译运行。契约（doc comment）与局部理由（普通注释）的分工由此不是团队约定而是语言级设计。Rust API guidelines 有一条对示例的深刻观察：

> "Like it or not, example code is often copied verbatim by users."
> （不管你乐不乐意，示例代码常被用户逐字照抄。）

实践中，ripgrep、tokio 与 std 的普通 `//` 高度集中在四类：性能取舍（「这里 Relaxed 排序就够，因为……」）、平台与工具链 hack（「用 raw-dylib 导入，因为不能指望 import library 存在」）、不变量（空匹配推进规则、并发序）、带外部依据的取舍（放宽严格条件并附 issue 依据）。此外 `// SAFETY:` 是生态最强的普通注释惯例——每个 unsafe 块前一行安全论证，clippy 可强制。

## 四、把判断做细的思想工具

谱系告诉人各家的位置，下面这些是各家用来做细粒度判断的工具：

**Ousterhout 的层级论**："Comments augment the code by providing information at a different level of detail." 好注释与代码不在同一层——高一层给直觉（这整块在干什么、为什么走这个方向），或低一层给精确（单位、哨兵值、并发前提）；与代码同层的复述是最没价值的形态。

**McConnell 的诊断器**（《Code Complete》）："If some code is difficult to comment, either it's bad code or you don't understand it well enough." 注释难度是代码质量的探测器——写不出、写不好注释，几乎总是代码的病：契约注释写不简洁，是抽象切口不对；注释在解释实现步骤而非约束，是实现太绕；注释在解释名字，是名字该改。

**antirez 的认知负荷论**：一条注释值不值得存在，看它是否降低了读者的总认知负荷——"Many comments don't explain what the code is doing. They explain what you can't understand just from what the code does."（很多注释不是解释代码在做什么，而是解释你光看代码了解不到的东西）。

**kernel 的分工论**：what、how、why 三个问题各有归宿——结构（命名、拆分）承担 how，函数头承担 what 与 why，体内注释只在聪明或丑陋之处点到即止。

## 五、我们的立场

**谱系位置**：本仓库站在 Linux 内核与 Google 一侧——契约层写足，复述层零容忍。这包含对 Ousterhout 两条主张的吸收：接口注释先于实现动笔（它是设计工具，不是事后负担），以及过期注释零容忍（改代码的同一个 commit 就地同步注释——"Comments belong to the code, not the commit log"，注释属于代码，不属于提交日志）。

**我们的语境与两家不同**：这个仓库的主要读者里包括 AI 编码代理。对人类读者，一条指向 issue 编号或设计文档的注释尚可原谅；对代理读者，那是幻觉的入口——它没有那些上下文，却会认真地去猜。因此本文将「自包含」置于比生态惯例更重的位置：注释不引用内部编号、暗语或文档路径，确有「某文档讲过」的必要时，把结论文本拷贝进来。这条比生态常见做法更严（生态里引用 bug 号很常规），是有意为之的选择，其代价是注释略长，收益是每条注释独立成立。

**一条付过学费的教训**：注释声称「运行时不可变 / by-design」，实际是未接线的 stub——这样的过期注释带着权威性误导过多轮排查。判断框架能力以 core 与 FFI 源码为准；而正确的方向，是让注释从源头上不漂移到需要这句免责。

最后回到那个行业最大公约数，作为本文的收束——非显而易见之处，写；显而易见之处，让代码自己说话。剩下的一切判断，属于落笔的人。

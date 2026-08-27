# Ikat 安装与初始化手册（AI 执行版）

> 读者：被指派「把 Ikat 装进这个 Unity 工程」的 AI 编码代理。人类读者走[手动路径](#人类手动路径gui)。
>
> 前提：Windows（打包器工具链只发布 Windows exe）；Unity 2021.3 LTS+；你对游戏仓库有写权限。

## 全景

两条线，**互相不阻塞**：

- **Unity 侧**：`manifest.json` 加一行 git URL → 用户开一次 Unity → 包（运行时 .dll、打包器 GUI、ikat CLI）自动装好。
- **工作区侧**：ikat CLI 从 GitHub Release 下载 → `ikat init` 落工作区（skills + 配置 + CLI 自拷贝）→ 示例包 `check` 转绿。

CLI 是单文件自足分发（skill 模板内嵌二进制），与 Unity 包内的 exe 同 tag 同构建——先下载先干活，不必等 Unity 装完。

## 第 0 步：确定版本 tag

查 Release 列表拿最新 tag（三选一，gh 未装/未认证走后两路）：

```
gh api repos/15wtyuan/Ikat/releases --jq '.[0].tag_name'
curl -s https://api.github.com/repos/15wtyuan/Ikat/releases | grep -m1 '"tag_name"'
```

或让用户打开 https://github.com/15wtyuan/Ikat/releases 看**最上面**一条。

⚠️ **不要用 `/releases/latest` 端点**：0.x 版本全部标记为 prerelease，该端点返回 404。

全程**单一 tag 原则**：manifest 里 pin 的 tag、下载的 exe、后续所有版本比对，全部对齐这一个 tag。注意格式差：tag 带 `v` 前缀（`v0.0.12`），而 `scaffold.version`、lock 文件、`ikat version` 输出的版本号**不带 `v`**——比对时先归一。

## 第 1 步：改 manifest

在 `<Unity 工程>/Packages/manifest.json` 的 `dependencies` 里加一行（tag 用第 0 步查到的）：

```json
"com.ikat.unity": "https://github.com/15wtyuan/Ikat.git?path=/unity/package#v0.0.12"
```

Unity 工程根的判定标志：含 `Assets/` 与 `Packages/manifest.json` 的目录。它可能就是仓库根，也可能是仓库里的子目录（如 `<repo>/client/`）——按实际位置来。

此时**不需要**用户开 Unity，装包留到第 6 步一次完成。

## 第 2 步：下载 ikat CLI

Release 主路（把 URL 里的 `v0.0.12` 换成第 0 步查到的 tag）：

```bash
curl -L --ssl-no-revoke -o ikat.exe https://github.com/15wtyuan/Ikat/releases/download/v0.0.12/ikat.exe
```

```powershell
# PowerShell 备用（Windows PowerShell 5.1 必须先开 TLS1.2；PowerShell 7 可省）：
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri 'https://github.com/15wtyuan/Ikat/releases/download/v0.0.12/ikat.exe' -OutFile 'ikat.exe'
```

（`--ssl-no-revoke` 针对 Git Bash 自带 curl 的 schannel 吊销检查失败；若仍失败直接换 PowerShell。）

下载后自检：`ikat.exe version` 应打印与 tag 一致的版本。不一致 → 删掉重下一次；再不一致 → URL 里的 tag 与第 0 步核对。init 之后这个临时副本就没用了（权威副本在 `.ikat/`），可删。

兜底路（Release 不可达时）：跳过本步，等第 7 步 Unity 装完包后从
`<Unity 工程>/Library/PackageCache/com.ikat.unity*/Editor/Tools/ikat.exe` 拷出使用。

## 第 3 步：问用户两件事——UI 目录与产物目录

全流程只有这两个必问项（一轮问完）：

1. **UI 工作区放哪**：建议默认 `<会话根>/ui`（独立于 Unity 工程源、入 git、团队共享）；用户有既定布局就听用户的。
2. **构建产物目录（output_dir）放哪**：Unity 工程内、相对工程根的路径，`ikat build`
   把 `.pkg.bin` / 图集 / 字体写到这里。建议默认 `Assets/Bundles`；团队已有自己的
   资源组织习惯（如 `Assets/Res/UI`、`Assets/AssetBundles`）就用既有的。

不要问其它问题：

- `.ikat/`（CLI + 配置）与 skills 目录建议入 git——产品设计的本意就是团队 clone 即得配套工具链，除非用户明确不想。

## 第 4 步：初始化工作区

会话根 = **当前 agent 会话打开的目录**——用户在哪开的会话，`.ikat/` 与 skills 就
落在哪（通常用户就在游戏仓库根开会话，所以通常即仓库根）。init 的 `<dir>` 参数
直接传它，不要另选目录。

```bash
ikat.exe init <会话根> --ui <UI 目录> --unity-root <Unity 工程根> --output <产物目录> --agent agents
```

- `--ui` 相对会话根解析。
- ⚠️ **必须带 `--output`（第 3 步问到的答案）**：CLI 的 `output_dir` 裸默认是 `dist`
  （落 Unity 工程外面，Unity 永远看不到）。该路径相对 unity 工程根解析。
- `--agent`：按你所在的宿主工具选——通用 `.agents/skills/` 传 `agents`（默认），
  Claude Code 传 `claude`（落 `.claude/skills/`），两者都要就重复传。
- 已有 `ikat.workspace.json` 时 init 会拒绝；`--force` 会把 workspace 重置回空骨架
  （已注册的包全丢），除非确知工作区是空的，否则不要 `--force`。

产出：`<UI 目录>/ikat.workspace.json` 骨架、`.ikat/`（config 双指针 + CLI 自拷贝 +
`scaffold.version` 版本戳）、skills 目录。此后统一用 `.ikat/ikat.exe`，不再依赖下载的临时副本。

## 第 5 步：示例包 + 验证

以下命令都在**会话根**执行（`.ikat/` 前缀即此约定；在别的目录跑就给绝对路径）。

```bash
.ikat/ikat.exe new demo
.ikat/ikat.exe check --format json
```

`check` 退出码 0 即安装验证通过。不需要字体文件（字体是写真实 UI 时才要的，届时
`ikat font add`，工作区里的 ikat skill 有完整指引）。

预期落点（`new` 把包源放在 UI 工作区**内层的 `ui/` 子目录**下——工作区根自带的源码
目录约定，不是你 init 错了）：`<UI 目录>/ui/demo/main.html`。

可选加深验证：`.ikat/ikat.exe build`，判据 = 退出码 0 且 stdout 的 `report.log` 含
写入路径（默认产物目录下为 `Assets/Bundles/ikat.runtime.json` 与
`Assets/Bundles/ui/demo.pkg.bin`）——产物落进 Unity 工程，「工作区 → Unity」链路即通。

## 第 6 步：请用户打开 Unity

告诉用户：切到（或打开）Unity 工程，右下角 Package Manager 会拉取 `com.ikat.unity`，
拉完即可。用户不需要做任何菜单操作。

## 第 7 步：验证 Unity 侧安装（文件系统轮询）

你没有 Unity 控制工具，验证天花板是文件系统。轮询两个信号（git 包拉取可能要几十秒；
建议每 10 秒查一次，**上限 5 分钟**）：

```bash
ls <Unity 工程>/Library/PackageCache/com.ikat.unity*/Editor/Tools/ikat.exe   # 信号 1：包落盘
grep -A2 '"com.ikat.unity"' <Unity 工程>/Packages/packages-lock.json        # 信号 2：版本入锁
```

信号 2 里的 `"version"` 应与你 pin 的 tag 版本号一致（去 `v` 前缀后比；git 包的版本号
取自 tag 指向 commit 的 package.json）。

超时未见 → 先看 Unity 的 Package Manager 窗口/Console 有无报错，再核对 manifest 里
git URL 的仓库/path/tag 拼写；网络慢就再等，不要无限轮询。

出现即装好。最后做一次**三方对齐检查**：manifest 的 tag ↔ lock 文件的 version ↔
`.ikat/scaffold.version`，三者（归一 `v` 前缀后）应指向同一版本。

## 第 8 步：汇报

向用户汇报：装好了；工作区在哪、会话根在哪；示例包 check 已绿；UI 开发循环怎么开始
（skills 已就位——写 UI 前读 ikat-editor（围栏规则）与 ikat（CLI 操作），运行时
C# API 查 ikat-runtime，它们是权威操作手册）。
顺带告知：偏好 GUI 的队友可走 Unity 菜单 `Ikat > Open Packer`。

## 日常版本升级

升级是使用期操作，权威指引已落进工作区的 **ikat skill「Version sync」节**（随包分发）。
要点：比对 `.ikat/scaffold.version` 与 packages-lock.json 的包版本发现漂移 → 用新
exe 覆盖 `.ikat/ikat.exe`（来源：PackageCache 拷出，或按新 tag 重新走第 2 步下载）→
跑 `ikat scaffold` 刷新 skills 与版本戳。

## 人类手动路径（GUI）

1. `manifest.json` 加行（同第 1 步），打开 Unity 等装包。
2. Unity 菜单 `Ikat > Open Packer` 打开打包器 GUI。
3. GUI 里完成工作区创建（选目录；输出目录默认 `Assets/Bundles`；skills 与 `.ikat/` 由 GUI 落好）。

## FAQ

- **`/releases/latest` 404** — 0.x 全是 prerelease，该端点只返正式版。用 Release 列表页/列表端点。
- **build 产物落在 `dist/` 而不是 `Assets/`** — init 时没带 `--output`（第 3 步问到的产物目录），见第 4 步。
- **Unity 工程挪位 / 仓库挪位后命令报 exit 2** — `.ikat/config.json` 的指针失配。它是纯指针文件（不是
  `ikat.workspace.json`，后者禁手改），直接手改 `ui_root` / `unity_root` 两个相对路径即可。
- **PackageCache 里找不到包** — 目录名带 hash 后缀，`com.ikat.unity*` 通配；若仍没有，说明 Unity
  还没拉包（回到第 6 步等用户），或 manifest 的 git URL 写错。
- **git URL 拉包慢/失败** — 大仓全量拉取，耐心或配置镜像；Unity 日志窗口有具体网络错误。

//! 会话根脚手架：四个 agent skills（ikat-editor 围栏规则手册 / ikat-runtime
//! 运行时接线手册 / ikat-preview 预览模拟手册 / ikat CLI 手册），从模板覆盖拷入。
//! CLI `ikat init` / `ikat scaffold` 与 GUI「新建工作区」共用（模板只此一份，防两端口径
//! 漂移）。模板文件在 crate 的 templates/ 下（include_str! 嵌进二进制——CLI 单 exe 分发）。

use std::path::Path;

/// 单个 skill 的落位产物：目录名 + (相对路径, 内容) 清单。
struct SkillArtifacts {
    dir: &'static str,
    files: Vec<(&'static str, &'static str)>,
}

/// editor skill 的深度参考表（渐进披露：SKILL.md 是操作手册，查找表按需加载）。
/// 元组首项是相对 skill 目录的路径（含 references/ 前缀）。
const EDITOR_REFERENCES: &[(&str, &str)] = &[
    (
        "references/fence-schema.md",
        include_str!("../templates/editor/references/fence-schema.md"),
    ),
    (
        "references/css-reference.md",
        include_str!("../templates/editor/references/css-reference.md"),
    ),
    (
        "references/patterns.md",
        include_str!("../templates/editor/references/patterns.md"),
    ),
];

/// runtime skill 的深度参考表（渐进披露：SKILL.md 是接线手册，API 查找表按需
/// 加载）。内容镜像随包 C# 签名契约——消费者 agent 查 API 不需要 Ikat 源码仓库。
const RUNTIME_REFERENCES: &[(&str, &str)] = &[(
    "references/api-reference.md",
    include_str!("../templates/runtime/references/api-reference.md"),
)];

/// preview skill 的深度参考表（可拷贝 recipe：组件展开 / 各控件模拟 / 演示数据）。
const PREVIEW_REFERENCES: &[(&str, &str)] = &[(
    "references/recipes.md",
    include_str!("../templates/preview/references/recipes.md"),
)];

fn skill_artifacts() -> Vec<SkillArtifacts> {
    vec![
        SkillArtifacts {
            dir: "ikat-editor",
            files: vec![("SKILL.md", include_str!("../templates/editor/SKILL.md"))],
        },
        SkillArtifacts {
            dir: "ikat-runtime",
            files: vec![("SKILL.md", include_str!("../templates/runtime/SKILL.md"))],
        },
        SkillArtifacts {
            dir: "ikat-preview",
            files: vec![("SKILL.md", include_str!("../templates/preview/SKILL.md"))],
        },
        SkillArtifacts {
            dir: "ikat",
            files: vec![("SKILL.md", include_str!("../templates/ikat/SKILL.md"))],
        },
    ]
}

/// 按 agent 类型写 skills 到会话根：`claude` 落 `.claude/skills/`，`agents` 落
/// `.agents/skills/`（AGENTS.md 约定的通用目录）。覆盖拷入（模板升级后重跑
/// init / scaffold 即刷新），不生成任何指令文档——skill 描述句负责触发，
/// 内容三件全在 skill 目录内。不碰 workspace.json 和源文件。
pub fn write_agent_scaffold(root: &Path, agents: &[String]) -> Result<(), String> {
    if agents.is_empty() {
        return Err("no agent kind selected (expected `claude` and/or `agents`)".to_string());
    }
    let mut skills = skill_artifacts();
    // references/ 并入对应 skill 的产物清单（editor 三件 + runtime API 查找表 +
    // preview recipe 查找表）。
    skills[0].files.extend_from_slice(EDITOR_REFERENCES);
    skills[1].files.extend_from_slice(RUNTIME_REFERENCES);
    skills[2].files.extend_from_slice(PREVIEW_REFERENCES);

    for agent in agents {
        let skills_dir = match agent.as_str() {
            "claude" => ".claude/skills",
            "agents" => ".agents/skills",
            other => return Err(format!("unknown agent kind: {other}")),
        };
        for skill in &skills {
            let dir = root.join(skills_dir).join(skill.dir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            for (rel, content) in &skill.files {
                if let Some(parent) = dir.join(rel).parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("create {}: {e}", parent.display()))?;
                }
                std::fs::write(dir.join(rel), content)
                    .map_err(|e| format!("write {}: {e}", dir.join(rel).display()))?;
            }
        }
    }
    Ok(())
}

/// ikat CLI 版本（`.ikat/scaffold.version` 戳与 check 的 stale 比对共用）。
pub const IKAT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 版本戳文件名（`.ikat/` 下）。
pub const VERSION_STAMP: &str = "scaffold.version";

/// 探测会话根既有的 scaffold agent 目录（.agents/.claude 的 skills 落位痕迹）。
/// 都没有（首次）默认 `agents`。刷新语义按「在场者」——不给 `--agent` 时
/// claude 用户工作区不会丢 `.claude/skills` 的更新。
pub fn detect_agents(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for (dir, name) in [(".agents/skills", "agents"), (".claude/skills", "claude")] {
        if root.join(dir).join("ikat-editor").is_dir() {
            out.push(name.to_string());
        }
    }
    if out.is_empty() {
        out.push("agents".to_string());
    }
    out
}

/// `ikat scaffold` 的刷新产物摘要。
#[derive(Debug)]
pub struct RefreshOutcome {
    /// 刷新的 agent 目录（.agents/.claude）。
    pub agents: Vec<String>,
    /// `.ikat/ikat(.exe)` 是否被更新（同文件自跑 = 已一致，跳过）。
    pub cli_updated: bool,
}

/// 刷新会话根的全部「生成物」：三 skill 覆盖重写 + `.ikat/ikat(.exe)` 自拷贝 +
/// 版本戳。白名单式——`config.json` / `ikat.workspace.json` / HTML/CSS/资源一概
/// 不碰。消费端更新流：UPM 更新包（Editor/Tools 新双 exe 落地）→ 跑一次本命令
/// （任意位置的 ikat.exe 均可，exe 以运行者为准）。
pub fn refresh_workspace(root: &Path, agents: &[String]) -> Result<RefreshOutcome, String> {
    write_agent_scaffold(root, agents)?;
    let ikat_dir = root.join(".ikat");
    std::fs::create_dir_all(&ikat_dir)
        .map_err(|e| format!("create {}: {e}", ikat_dir.display()))?;

    // CLI 自拷贝：目标即运行中的 exe（同路径）时跳过——本命令就是它产出的版本。
    // Windows 锁运行中 exe 的写入，但不同路径拷贝不受影响；极端并发占用按失败降级。
    let cli_updated = match std::env::current_exe() {
        Ok(src) if src.is_file() => {
            let dst = ikat_dir.join(src.file_name().unwrap_or_default());
            let same = std::path::absolute(&dst).ok() == std::path::absolute(&src).ok();
            if same {
                false
            } else if std::fs::copy(&src, &dst).is_ok() {
                true
            } else {
                let _ = std::fs::remove_file(&dst);
                std::fs::copy(&src, &dst).is_ok()
            }
        }
        _ => false,
    };

    std::fs::write(ikat_dir.join(VERSION_STAMP), IKAT_VERSION)
        .map_err(|e| format!("write version stamp: {e}"))?;
    Ok(RefreshOutcome {
        agents: agents.to_vec(),
        cli_updated,
    })
}

/// 工作区生成物是否落后于本 CLI：从 `dir` 向上找 `.ikat/scaffold.version`（最多
/// 两级，对应会话根/ui 两种摆放），与 [`IKAT_VERSION`] 比对。None = 一致或无戳
/// （无 `.ikat` 的裸工作区不打扰）。
pub fn stale_stamp_warning(dir: &Path) -> Option<String> {
    let mut cur = Some(dir);
    for _ in 0..3 {
        let Some(d) = cur else { break };
        let stamp = d.join(".ikat").join(VERSION_STAMP);
        if let Ok(v) = std::fs::read_to_string(&stamp) {
            let v = v.trim();
            if v != IKAT_VERSION {
                return Some(format!(
                    "workspace scaffold is from ikat {v} but this ikat is {IKAT_VERSION} — \
                     run `ikat scaffold` to refresh skills and the pinned CLI",
                ));
            }
            return None;
        }
        cur = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脚手架产物契约：三 skill + editor references 落位；无指令文档；未知 kind 拒绝。
    #[test]
    fn scaffold_writes_three_skills_per_agent() {
        let tmp = std::env::temp_dir().join(format!("ikat_scaffold_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_agent_scaffold(&tmp, &["claude".to_string(), "agents".to_string()]).unwrap();

        for skills in [".claude/skills", ".agents/skills"] {
            assert!(
                tmp.join(skills).join("ikat-editor/SKILL.md").is_file(),
                "{skills} 的 editor skill 须落位"
            );
            assert!(
                tmp.join(skills)
                    .join("ikat-editor/references/fence-schema.md")
                    .is_file(),
                "{skills} 的 editor references 须落位"
            );
            assert!(
                tmp.join(skills).join("ikat-runtime/SKILL.md").is_file(),
                "{skills} 的 runtime skill 须落位"
            );
            assert!(
                tmp.join(skills)
                    .join("ikat-runtime/references/api-reference.md")
                    .is_file(),
                "{skills} 的 runtime references 须落位"
            );
            let ikat = std::fs::read_to_string(tmp.join(skills).join("ikat/SKILL.md")).unwrap();
            assert!(ikat.contains("ikat check"), "ikat skill 须含命令面");
        }
        // 不再生成指令文档（AGENTS.md / CLAUDE.md 由用户自持）。
        assert!(!tmp.join("AGENTS.md").exists());
        assert!(!tmp.join("CLAUDE.md").exists());
        // 未知 agent kind 拒绝。
        assert!(write_agent_scaffold(&tmp, &["vscode".to_string()]).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

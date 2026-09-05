// 暴露 Yio.Runtime 的 internal 给测试程序集：KeyListAlignmentTests 校验
// YioInputCollector 的平行键表（KeyList/NewKeyList，新旧输入路径同下标配对）。
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo("Yio.Tests")]

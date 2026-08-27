// 暴露 Ikat.Runtime 的 internal 给测试程序集：KeyListAlignmentTests 校验
// IkatInputCollector 的平行键表（KeyList/NewKeyList，新旧输入路径同下标配对）。
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo("Ikat.Tests")]

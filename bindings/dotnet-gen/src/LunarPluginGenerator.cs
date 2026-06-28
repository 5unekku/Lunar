using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Text;

/// <summary>
/// generates the <c>[UnmanagedCallersOnly]</c> entry point for any class tagged
/// <c>[LunarPlugin]</c>. exactly one such class per assembly is expected.
/// </summary>
[Generator(LanguageNames.CSharp)]
public sealed class LunarPluginGenerator : IIncrementalGenerator
{
    const string AttributeFqn = "Lunar.LunarPluginAttribute";
    const string ExportAttributeFqn = "Lunar.ExportAttribute";

    public void Initialize(IncrementalGeneratorInitializationContext context)
    {
        var plugins = context.SyntaxProvider
            .ForAttributeWithMetadataName(
                AttributeFqn,
                predicate: static (node, _) => node is ClassDeclarationSyntax,
                transform: static (ctx, _) => ctx.TargetSymbol as INamedTypeSymbol)
            .Where(static symbol => symbol is not null)
            .Collect();

        context.RegisterSourceOutput(plugins, Emit);

        // collect every [Export]-tagged member (field or property), grouped per behavior type
        var exports = context.SyntaxProvider
            .ForAttributeWithMetadataName(
                ExportAttributeFqn,
                predicate: static (node, _) => true,
                transform: static (ctx, _) => (ISymbol?)ctx.TargetSymbol)
            .Where(static symbol => symbol is not null)
            .Collect();

        context.RegisterSourceOutput(exports, EmitExports);
    }

    /// <summary>one exported member: its name and the FieldKind it maps to.</summary>
    readonly struct ExportedMember
    {
        public readonly string Name;
        public readonly string Kind;
        public readonly string GetExpr;
        public readonly string SetStmt;

        public ExportedMember(string name, string kind, string getExpr, string setStmt)
        {
            Name = name;
            Kind = kind;
            GetExpr = getExpr;
            SetStmt = setStmt;
        }
    }

    static void EmitExports(SourceProductionContext ctx, ImmutableArray<ISymbol?> members)
    {
        if (members.IsDefaultOrEmpty) return;

        // group members by their containing behavior type
        var byType = new Dictionary<INamedTypeSymbol, List<ExportedMember>>(SymbolEqualityComparer.Default);
        foreach (var member in members)
        {
            if (member is null) continue;
            var owner = member.ContainingType;
            if (owner is null) continue;

            var (typeText, ok) = MemberType(member);
            if (!ok) continue;
            var mapped = MapKind(typeText, member.Name);
            if (mapped is null) continue;

            if (!byType.TryGetValue(owner, out var list))
            {
                list = new List<ExportedMember>();
                byType[owner] = list;
            }
            list.Add(mapped.Value);
        }

        foreach (var pair in byType)
            EmitBehaviorFields(ctx, pair.Key, pair.Value);
    }

    static (string, bool) MemberType(ISymbol member) => member switch
    {
        IFieldSymbol field => (field.Type.ToDisplayString(), true),
        IPropertySymbol property => (property.Type.ToDisplayString(), true),
        _ => ("", false),
    };

    /// <summary>map a C# type name to (FieldKind, get expression, set statement) for one member.</summary>
    static ExportedMember? MapKind(string typeText, string name)
    {
        switch (typeText)
        {
            case "float":
            case "System.Single":
                return new ExportedMember(name, "Float",
                    $"global::Lunar.FieldValue.OfFloat(this.{name})", $"this.{name} = value.Float;");
            case "double":
            case "System.Double":
                return new ExportedMember(name, "Float",
                    $"global::Lunar.FieldValue.OfFloat((float)this.{name})", $"this.{name} = value.Float;");
            case "int":
            case "System.Int32":
                return new ExportedMember(name, "Int",
                    $"global::Lunar.FieldValue.OfInt(this.{name})", $"this.{name} = (int)value.Int;");
            case "long":
            case "System.Int64":
                return new ExportedMember(name, "Int",
                    $"global::Lunar.FieldValue.OfInt(this.{name})", $"this.{name} = value.Int;");
            case "bool":
            case "System.Boolean":
                return new ExportedMember(name, "Bool",
                    $"global::Lunar.FieldValue.OfBool(this.{name})", $"this.{name} = value.Bool;");
            case "string":
            case "string?":
            case "System.String":
                return new ExportedMember(name, "Text",
                    $"global::Lunar.FieldValue.OfText(this.{name})", $"this.{name} = value.Text;");
            default:
                return null;
        }
    }

    static void EmitBehaviorFields(
        SourceProductionContext ctx, INamedTypeSymbol type, List<ExportedMember> members)
    {
        var ns = type.ContainingNamespace.IsGlobalNamespace
            ? string.Empty
            : type.ContainingNamespace.ToDisplayString();

        var source = new StringBuilder();
        source.AppendLine("// <auto-generated/>");
        source.AppendLine("#nullable enable");
        if (ns.Length > 0)
        {
            source.AppendLine($"namespace {ns};");
            source.AppendLine();
        }
        source.AppendLine($"partial class {type.Name}");
        source.AppendLine("{");
        source.AppendLine($"    public int FieldCount => {members.Count};");
        source.AppendLine();
        source.AppendLine("    public bool GetFieldSchema(int index, out string name, out global::Lunar.FieldKind kind)");
        source.AppendLine("    {");
        source.AppendLine("        switch (index)");
        source.AppendLine("        {");
        for (int i = 0; i < members.Count; i++)
            source.AppendLine($"            case {i}: name = \"{members[i].Name}\"; kind = global::Lunar.FieldKind.{members[i].Kind}; return true;");
        source.AppendLine("            default: name = \"\"; kind = global::Lunar.FieldKind.Float; return false;");
        source.AppendLine("        }");
        source.AppendLine("    }");
        source.AppendLine();
        source.AppendLine("    public bool GetField(string name, out global::Lunar.FieldValue value)");
        source.AppendLine("    {");
        source.AppendLine("        switch (name)");
        source.AppendLine("        {");
        foreach (var member in members)
            source.AppendLine($"            case \"{member.Name}\": value = {member.GetExpr}; return true;");
        source.AppendLine("            default: value = global::Lunar.FieldValue.OfFloat(0); return false;");
        source.AppendLine("        }");
        source.AppendLine("    }");
        source.AppendLine();
        source.AppendLine("    public void SetField(string name, global::Lunar.FieldValue value)");
        source.AppendLine("    {");
        source.AppendLine("        switch (name)");
        source.AppendLine("        {");
        foreach (var member in members)
            source.AppendLine($"            case \"{member.Name}\": {member.SetStmt} break;");
        source.AppendLine("        }");
        source.AppendLine("    }");
        source.AppendLine("}");

        var hint = ns.Length > 0 ? $"{ns}.{type.Name}.Behavior.g.cs" : $"{type.Name}.Behavior.g.cs";
        ctx.AddSource(hint, source.ToString());
    }

    static void Emit(SourceProductionContext ctx, ImmutableArray<INamedTypeSymbol?> symbols)
    {
        if (symbols.IsDefaultOrEmpty) return;

        // report diagnostic for duplicates
        for (int i = 1; i < symbols.Length; i++)
        {
            if (symbols[i] is not { } extra) continue;
            ctx.ReportDiagnostic(Diagnostic.Create(
                new DiagnosticDescriptor(
                    id: "LUNAR001",
                    title: "multiple [LunarPlugin] classes",
                    messageFormat: "only one [LunarPlugin] class per assembly is allowed; '{0}' is ignored",
                    category: "LunarPlugin",
                    defaultSeverity: DiagnosticSeverity.Warning,
                    isEnabledByDefault: true),
                location: null,
                extra.ToDisplayString()));
        }

        if (symbols[0] is not { } symbol) return;

        var ns = symbol.ContainingNamespace.IsGlobalNamespace
            ? string.Empty
            : symbol.ContainingNamespace.ToDisplayString();
        var qualifiedName = ns.Length > 0 ? $"{ns}.{symbol.Name}" : symbol.Name;

        var source = new StringBuilder();
        source.AppendLine("// <auto-generated/>");
        source.AppendLine("using System.Runtime.CompilerServices;");
        source.AppendLine("using System.Runtime.InteropServices;");
        source.AppendLine("using Lunar;");
        source.AppendLine();
        source.AppendLine($"// NativeAOT path: called by Rust via dlopen + lunar_plugin_init symbol");
        source.AppendLine("file static class LunarGeneratedEntryPoint");
        source.AppendLine("{");
        source.AppendLine("    [UnmanagedCallersOnly(EntryPoint = \"lunar_plugin_init\", CallConvs = [typeof(CallConvCdecl)])]");
        source.AppendLine($"    public static unsafe void Init(void* world) => Plugin.Run(world, new {qualifiedName}());");
        source.AppendLine("}");
        source.AppendLine();
        source.AppendLine("// CoreCLR path: called by LunarHost bootstrapper via reflection");
        source.AppendLine("// internal so it's visible to reflection from any assembly");
        source.AppendLine("internal static class LunarGeneratedHost");
        source.AppendLine("{");
        source.AppendLine($"    public static unsafe void ManagedInit(nint worldPtr) => Plugin.Run((void*)worldPtr, new {qualifiedName}());");
        source.AppendLine("}");

        ctx.AddSource("LunarPlugin.g.cs", source.ToString());
    }
}

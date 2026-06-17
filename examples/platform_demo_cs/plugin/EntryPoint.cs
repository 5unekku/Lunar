using Lunar;

namespace PlatformDemoCs;

[LunarPlugin]
class GamePlugin : IPlugin
{
    static readonly Color GrassColor = new(0.22f, 0.52f, 0.09f);
    static readonly Color SkyColor   = new(0.40f, 0.65f, 1.00f);
    static readonly Color SunColor   = new(1.00f, 0.98f, 0.85f);

    const float HalfPlatform = 2.0f;
    const float EyeHeight    = 1.7f;
    const float FovDegrees   = 90.0f;

    public void Init(World world)
    {
        world.SetCursorLocked(true);
        world.SetQuality(msaaSamples: 4, staa: true, renderScale: 1.0f);
        world.SetSky(SkyColor, SunColor, showSun: true);

        var floorMesh = world.CreateMeshQuad(HalfPlatform, HalfPlatform);
        var grassMat  = world.CreateMaterial(GrassColor, ShadingModel.Unlit);
        world.SpawnMesh(floorMesh, grassMat, 0.0f, 0.0f, 0.0f);

        float fovY = FovDegrees * (float)Math.PI / 180.0f;
        var camera = world.SpawnCamera(0.0f, EyeHeight, 0.0f, fovY, near: 0.1f, far: 1000.0f);
        world.SetActiveCamera(camera);

        world.RegisterSystem(LunarSchedule.Update, new FpsController());
    }
}

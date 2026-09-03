// SPIKE fixture: resolve how COMBINED depth-stencil maps manifest-descriptor ->
// fetched-aspect-streamRef (docs/findings/00-texture-fetch.md, the ordinal
// bridge / gputrace-bundle join). A combined Depth32Float_Stencil8
// resource is ONE manifest descriptor (format 260) but the fetch serves its
// aspects as SEPARATE textures under per-aspect formats (252 Depth32Float,
// 261 X32_Stencil8). The join's format-strict zip breaks on this; we have no
// measured ground truth for the correct correspondence.
//
// Two combined DS resources at DISTINCT dims (64x64 and 96x96) with distinct
// depth (0.25 vs 0.75) and stencil (11 vs 22), so every fetched aspect is
// identifiable by dims (which resource) + content (depth vs stencil aspect),
// and the manifest's two 260 descriptors are distinguishable by dims. The
// analysis then reads: how many descriptors, how many aspect streamRefs per
// resource, and whether store0-offset rank predicts the aspect streamRef rank.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-ds-pair \
//         fixture-apps/known-ds-pair.m -framework Metal -framework Foundation
// Capture:
//   fixture-apps/capture-late.sh /tmp/known-ds-pair captures/known-ds-pair.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <unistd.h>

static NSString *const kSource =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"vertex float4 v_main(uint vid [[vertex_id]], constant float &z [[buffer(0)]]) {\n"
    @"    float2 p[3] = { float2(-1,-3), float2(-1,1), float2(3,1) };\n"
    @"    return float4(p[vid], z, 1.0);\n"
    @"}\n"
    @"fragment float4 f_main() { return float4(1,1,1,1); }\n";

static id<MTLRenderPipelineState> g_pso;
static id<MTLDepthStencilState> g_dss;

// One combined-DS resource: three textures ALLOCATED ONCE (color render target,
// the DS render target `ds_src`, and the blit-stored `ds_dst`). Phase 1 and
// phase 2 both re-render into these SAME objects, so the captured resource holds
// real stored content (allocating fresh each phase leaves the fetched bytes
// uninitialised - the earlier bug this fixture had).
typedef struct { id<MTLTexture> color, ds_src, ds_dst; NSUInteger w, h; } DsRes;

static DsRes alloc_ds(id<MTLDevice> device, NSUInteger W, NSUInteger H, const char *tag) {
    const MTLPixelFormat DSFMT = MTLPixelFormatDepth32Float_Stencil8;
    MTLTextureDescriptor *cd = [MTLTextureDescriptor
        texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm width:W height:H mipmapped:NO];
    cd.usage = MTLTextureUsageRenderTarget; cd.storageMode = MTLStorageModePrivate;
    id<MTLTexture> color = [device newTextureWithDescriptor:cd];
    MTLTextureDescriptor *dd = [MTLTextureDescriptor
        texture2DDescriptorWithPixelFormat:DSFMT width:W height:H mipmapped:NO];
    dd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
    dd.storageMode = MTLStorageModePrivate;
    id<MTLTexture> ds_src = [device newTextureWithDescriptor:dd];
    dd.usage = MTLTextureUsageRenderTarget;
    id<MTLTexture> ds_dst = [device newTextureWithDescriptor:dd];
    ds_src.label = [NSString stringWithFormat:@"ds_src_%s", tag];
    ds_dst.label = [NSString stringWithFormat:@"ds_dst_%s", tag];
    return (DsRes){ color, ds_src, ds_dst, W, H };
}

// Render `depth` + stencil `ref` into r.ds_src, then blit-store into r.ds_dst.
static void render_ds(id<MTLCommandQueue> queue, DsRes r, float depth, uint32_t ref) {
    id<MTLTexture> color = r.color, ds_src = r.ds_src, ds_dst = r.ds_dst;
    NSUInteger W = r.w, H = r.h;
    MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
    rp.colorAttachments[0].texture = color;
    rp.colorAttachments[0].loadAction = MTLLoadActionClear;
    rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 1);
    rp.colorAttachments[0].storeAction = MTLStoreActionStore;
    rp.depthAttachment.texture = ds_src;
    rp.depthAttachment.loadAction = MTLLoadActionClear;
    rp.depthAttachment.clearDepth = 1.0;
    rp.depthAttachment.storeAction = MTLStoreActionStore;
    rp.stencilAttachment.texture = ds_src;
    rp.stencilAttachment.loadAction = MTLLoadActionClear;
    rp.stencilAttachment.clearStencil = 0;
    rp.stencilAttachment.storeAction = MTLStoreActionStore;

    id<MTLCommandBuffer> cb = [queue commandBuffer];
    id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
    [enc setRenderPipelineState:g_pso];
    [enc setDepthStencilState:g_dss];
    [enc setStencilReferenceValue:ref];
    [enc setVertexBytes:&depth length:sizeof(depth) atIndex:0];
    [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
    [enc endEncoding];
    id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
    [blit copyFromTexture:ds_src sourceSlice:0 sourceLevel:0
             sourceOrigin:MTLOriginMake(0,0,0) sourceSize:MTLSizeMake(W,H,1)
                toTexture:ds_dst destinationSlice:0 destinationLevel:0
        destinationOrigin:MTLOriginMake(0,0,0)];
    [blit endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
    if (cb.error) fprintf(stderr, "cb error: %s\n", cb.error.localizedDescription.UTF8String);
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) { fprintf(stderr, "no device\n"); return 1; }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) { fprintf(stderr, "compile: %s\n", err.localizedDescription.UTF8String); return 1; }
        MTLRenderPipelineDescriptor *pd = [[MTLRenderPipelineDescriptor alloc] init];
        pd.vertexFunction = [lib newFunctionWithName:@"v_main"];
        pd.fragmentFunction = [lib newFunctionWithName:@"f_main"];
        pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        pd.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float_Stencil8;
        pd.stencilAttachmentPixelFormat = MTLPixelFormatDepth32Float_Stencil8;
        g_pso = [device newRenderPipelineStateWithDescriptor:pd error:&err];
        if (!g_pso) { fprintf(stderr, "pipeline: %s\n", err.localizedDescription.UTF8String); return 1; }

        MTLDepthStencilDescriptor *dsd = [[MTLDepthStencilDescriptor alloc] init];
        dsd.depthCompareFunction = MTLCompareFunctionAlways;
        dsd.depthWriteEnabled = YES;
        MTLStencilDescriptor *sc = [[MTLStencilDescriptor alloc] init];
        sc.stencilCompareFunction = MTLCompareFunctionAlways;
        sc.depthStencilPassOperation = MTLStencilOperationReplace;
        sc.writeMask = 0xFF;
        dsd.frontFaceStencil = sc;
        dsd.backFaceStencil = sc;
        g_dss = [device newDepthStencilStateWithDescriptor:dsd];

        // Allocate ONCE; both phases re-render into these same objects.
        DsRes a = alloc_ds(device, 64, 64, "A");   // depth 0.25, stencil 11
        DsRes b = alloc_ds(device, 96, 96, "B");    // depth 0.75, stencil 22
        void (^work)(void) = ^{
            render_ds(queue, a, 0.25f, 11);
            render_ds(queue, b, 0.75f, 22);
        };
        work();
        printf("phase 1: DS-A 64x64 depth0.25 stencil11 ; DS-B 96x96 depth0.75 stencil22\n");

        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("waiting for go-file %s\n", goFile); fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) { usleep(100000); if (++waited > 600) { fprintf(stderr, "no go-file\n"); return 1; } }
            work();
            printf("phase 2: re-rendered into the same objects inside capture\n");
        }
        printf("done (2 combined Depth32Float_Stencil8: %s + %s)\n",
               a.ds_dst.label.UTF8String, b.ds_dst.label.UTF8String);
    }
    return 0;
}

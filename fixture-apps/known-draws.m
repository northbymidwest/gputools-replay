// A fixture app for the dispatch-keyed WIREFRAME fetch class
// (GTReplayFetchWireframe): a render pass with several distinct draw calls, so
// a capture of it can be fetched by draw index and each draw's rendered
// wireframe image inspected.
//
// Unlike the resource-keyed fetches (buffer/heap/accel/pipeline, keyed by
// streamRef), wireframe fetch is keyed by dispatchUID - a small-integer draw
// index into the command stream (dossier 00). So this fixture issues N
// SEPARATE drawPrimitives calls into one render pass; each is a draw the
// replayer can re-render as a wireframe.
//
// Two-phase (late boundary), like known-buffers.m: the render target and the
// vertex buffer are created in phase 1, then phase 2 issues the draws inside
// the capture so they are captured commands over pre-existing resources.
//
// Build:
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-draws \
//         fixture-apps/known-draws.m -framework Metal -framework Foundation
//
// Capture (late boundary):
//   fixture-apps/capture-late.sh /tmp/known-draws captures/known-draws.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <unistd.h>

// Three triangles, one per draw call, at distinct positions so each rendered
// wireframe is non-blank and distinguishable. Tightly packed float2 clip-space
// positions, three vertices per triangle.
enum { kDrawCount = 3, kVertsPerTri = 3 };
static const float kVerts[kDrawCount][kVertsPerTri * 2] = {
    {-0.9f, -0.9f, -0.5f, -0.9f, -0.7f, -0.5f},  // lower-left
    {-0.1f, -0.1f, 0.4f, -0.1f, 0.15f, 0.5f},    // centre
    {0.5f, 0.5f, 0.9f, 0.5f, 0.7f, 0.9f},        // upper-right
};

static NSString *const kSource =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"struct VOut { float4 pos [[position]]; };\n"
    @"vertex VOut v_main(const device float2* verts [[buffer(0)]],\n"
    @"                   uint vid [[vertex_id]]) {\n"
    @"    VOut o; o.pos = float4(verts[vid], 0.0, 1.0); return o;\n"
    @"}\n"
    @"fragment float4 f_main() { return float4(1.0, 0.5, 0.0, 1.0); }\n";

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            fprintf(stderr, "known-draws: no Metal device\n");
            return 1;
        }
        printf("device: %s\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        // Offscreen render target and the vertex buffer, created in phase 1.
        MTLTextureDescriptor *td = [MTLTextureDescriptor
            texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                         width:256
                                        height:256
                                     mipmapped:NO];
        td.usage = MTLTextureUsageRenderTarget;
        td.storageMode = MTLStorageModePrivate;
        id<MTLTexture> rt = [device newTextureWithDescriptor:td];
        rt.label = @"draws_rt";

        id<MTLBuffer> vbuf = [device newBufferWithBytes:kVerts
                                                 length:sizeof(kVerts)
                                                options:MTLResourceStorageModeShared];
        vbuf.label = @"draws_verts";
        if (!rt || !vbuf) {
            fprintf(stderr, "known-draws: resource allocation failed\n");
            return 1;
        }

        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) {
            fprintf(stderr, "known-draws: shader compile failed: %s\n",
                    err.localizedDescription.UTF8String);
            return 1;
        }
        MTLRenderPipelineDescriptor *pd = [[MTLRenderPipelineDescriptor alloc] init];
        pd.vertexFunction = [lib newFunctionWithName:@"v_main"];
        pd.fragmentFunction = [lib newFunctionWithName:@"f_main"];
        pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        id<MTLRenderPipelineState> pso =
            [device newRenderPipelineStateWithDescriptor:pd error:&err];
        if (!pso) {
            fprintf(stderr, "known-draws: pipeline failed: %s\n",
                    err.localizedDescription.UTF8String);
            return 1;
        }

        // Issue kDrawCount separate draws into one render pass.
        void (^render)(void) = ^{
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = rt;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 1);
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLRenderCommandEncoder> enc =
                [cb renderCommandEncoderWithDescriptor:rp];
            [enc setRenderPipelineState:pso];
            [enc setVertexBuffer:vbuf offset:0 atIndex:0];
            for (int d = 0; d < kDrawCount; d++) {
                [enc drawPrimitives:MTLPrimitiveTypeTriangle
                        vertexStart:d * kVertsPerTri
                        vertexCount:kVertsPerTri];
            }
            [enc endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
        };

        render();  // phase 1
        printf("phase 1: rendered %d draws\n", kDrawCount);

        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("phase 1 done; waiting for go-file %s\n", goFile);
            fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) {
                usleep(100000);
                if (++waited > 600) {
                    fprintf(stderr, "known-draws: go-file never appeared\n");
                    return 1;
                }
            }
            render();  // phase 2, inside the capture
            printf("phase 2: re-rendered %d draws inside the capture\n", kDrawCount);
        }
        printf("done\n");
    }
    return 0;
}

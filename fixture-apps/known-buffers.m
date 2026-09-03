// A fixture app with exact ground truth for the BUFFER and HEAP fetch classes,
// the analogue of known-textures.m for non-texture resources.
//
// Purpose. The safe crate's live smoke test covers textures only; the
// resource-keyed non-texture fetches (GTReplayFetchBuffer, GTReplayFetchHeap)
// had no ground-truth live coverage. This app creates a small, enumerated set
// of buffers with known contents and USES them in a compute dispatch, so a
// capture of it can be fetched and checked byte for byte.
//
// Why two-phase, and why "used". A resource the captured commands never read is
// an "unused resource" the replayer unloads and will not serve (dossier 00);
// and a resource created AND destroyed entirely inside a single-phase capture
// is not snapshotted for fetch either (measured: a capture.sh run of this app
// answers nothing). The working pattern, shared with accel-structure.m, is a
// LATE-boundary capture: create and fill the resources in phase 1, block until
// a go-file appears (the capture boundary is taken during the block), then
// re-run the work in phase 2 so every resource pre-exists the boundary AND is
// used by a captured command. Drive it with capture-late.sh.
//
// Ground truth. Three standalone buffers, distinct sizes so a reply record
// identifies its row by size, each filled with a distinct arithmetic pattern,
// plus one heap-allocated buffer so the heap is a used resource too:
//   in_a: 64  u32  (256 B)   in_a[i]   = i
//   in_b: 96  u32  (384 B)   in_b[i]   = 0x1000 + i
//   out : 128 u32  (512 B)   out[i]    = (i<64 ? in_a[i]*2 : 0)
//                                      + (i<96 ? in_b[i] : 0)
//                                      + (i<64 ? heap[i] : 0)
//   heap: 64  u32  (256 B)   heap[i]   = 0x2000 + i   (sub-allocated from a heap)
// in_a/in_b/heap are written CPU-side (shared storage); out is GPU-written.
//
// Build (no Xcode project, per fixture-apps/README.md):
//   clang -fobjc-arc -fmodules -O0 -o /tmp/known-buffers \
//         fixture-apps/known-buffers.m -framework Metal -framework Foundation
//
// Capture (LATE boundary - single-phase capture.sh answers nothing):
//   fixture-apps/capture-late.sh /tmp/known-buffers captures/known-buffers.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

// Element counts (u32) per buffer. Distinct so a reply record's size names the
// buffer. Keep in sync with fixture-apps/README.md and tests/live_buffers.rs.
enum {
    kInACount = 64,   // 256 bytes
    kInBCount = 96,   // 384 bytes
    kOutCount = 128,  // 512 bytes
    kHeapCount = 64,  // 256 bytes, heap-allocated
};

// The one compute kernel: reads all inputs, writes a deterministic result, so
// every buffer is a resource the captured commands actually read or write.
static NSString *const kSource =
    @"#include <metal_stdlib>\n"
    @"using namespace metal;\n"
    @"kernel void combine(device const uint* a [[buffer(0)]],\n"
    @"                    device const uint* b [[buffer(1)]],\n"
    @"                    device const uint* h [[buffer(2)]],\n"
    @"                    device uint* o       [[buffer(3)]],\n"
    @"                    uint gid [[thread_position_in_grid]]) {\n"
    @"    uint av = gid < 64u ? a[gid] * 2u : 0u;\n"
    @"    uint bv = gid < 96u ? b[gid] : 0u;\n"
    @"    uint hv = gid < 64u ? h[gid] : 0u;\n"
    @"    o[gid] = av + bv + hv;\n"
    @"}\n";

static void dispatch_combine(id<MTLCommandQueue> queue, id<MTLComputePipelineState> pso,
                             id<MTLBuffer> a, id<MTLBuffer> b, id<MTLBuffer> h,
                             id<MTLBuffer> o) {
    id<MTLCommandBuffer> cb = [queue commandBuffer];
    id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
    [enc setComputePipelineState:pso];
    [enc setBuffer:a offset:0 atIndex:0];
    [enc setBuffer:b offset:0 atIndex:1];
    [enc setBuffer:h offset:0 atIndex:2];
    [enc setBuffer:o offset:0 atIndex:3];
    [enc dispatchThreads:MTLSizeMake(kOutCount, 1, 1)
        threadsPerThreadgroup:MTLSizeMake(32, 1, 1)];
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
}

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            fprintf(stderr, "known-buffers: no Metal device\n");
            return 1;
        }
        printf("device: %s\n", device.name.UTF8String);

        id<MTLCommandQueue> queue = [device newCommandQueue];
        if (!queue) {
            fprintf(stderr, "known-buffers: no command queue\n");
            return 1;
        }

        const NSUInteger bytesA = kInACount * sizeof(uint32_t);
        const NSUInteger bytesB = kInBCount * sizeof(uint32_t);
        const NSUInteger bytesOut = kOutCount * sizeof(uint32_t);
        const NSUInteger bytesHeap = kHeapCount * sizeof(uint32_t);

        // Ground-truth input patterns, built CPU-side.
        uint32_t patA[kInACount], patB[kInBCount], patHeap[kHeapCount];
        for (uint32_t i = 0; i < kInACount; i++) patA[i] = i;
        for (uint32_t i = 0; i < kInBCount; i++) patB[i] = 0x1000u + i;
        for (uint32_t i = 0; i < kHeapCount; i++) patHeap[i] = 0x2000u + i;

        // Standalone buffers: top-level resources GTReplayFetchBuffer serves.
        id<MTLBuffer> inA = [device newBufferWithBytes:patA
                                                length:bytesA
                                               options:MTLResourceStorageModeShared];
        id<MTLBuffer> inB = [device newBufferWithBytes:patB
                                                length:bytesB
                                               options:MTLResourceStorageModeShared];
        id<MTLBuffer> out = [device newBufferWithLength:bytesOut
                                                options:MTLResourceStorageModeShared];
        inA.label = @"known_in_a";
        inB.label = @"known_in_b";
        out.label = @"known_out";

        // One heap with one sub-allocated buffer, so the heap is a used resource.
        MTLHeapDescriptor *hd = [[MTLHeapDescriptor alloc] init];
        hd.storageMode = MTLStorageModeShared;
        hd.size = 64 * 1024;
        id<MTLHeap> heap = [device newHeapWithDescriptor:hd];
        id<MTLBuffer> heapBuf = [heap newBufferWithLength:bytesHeap
                                                  options:MTLResourceStorageModeShared];
        heapBuf.label = @"known_heap_buf";
        if (!inA || !inB || !out || !heap || !heapBuf) {
            fprintf(stderr, "known-buffers: resource allocation failed\n");
            return 1;
        }
        memcpy(heapBuf.contents, patHeap, bytesHeap);
        memset(out.contents, 0, bytesOut);

        // Compile the kernel and build the compute pipeline.
        NSError *err = nil;
        id<MTLLibrary> lib = [device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) {
            fprintf(stderr, "known-buffers: shader compile failed: %s\n",
                    err.localizedDescription.UTF8String);
            return 1;
        }
        id<MTLFunction> fn = [lib newFunctionWithName:@"combine"];
        id<MTLComputePipelineState> pso =
            [device newComputePipelineStateWithFunction:fn error:&err];
        if (!pso) {
            fprintf(stderr, "known-buffers: pipeline failed: %s\n",
                    err.localizedDescription.UTF8String);
            return 1;
        }

        // Phase 1: use every buffer once, so they exist and are filled.
        dispatch_combine(queue, pso, inA, inB, heapBuf, out);

        uint32_t *o = (uint32_t *)out.contents;
        printf("phase 1: in_a[10]=%u in_b[10]=0x%x heap[10]=0x%x out[10]=%u out[70]=%u\n",
               ((uint32_t *)inA.contents)[10], ((uint32_t *)inB.contents)[10],
               ((uint32_t *)heapBuf.contents)[10], o[10], o[70]);

        // Phase 2 (late-boundary capture): block for the go-file, then re-run
        // the dispatch inside the capture so every buffer pre-exists the
        // boundary AND is used by a captured command.
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            printf("phase 1 done; waiting for go-file %s\n", goFile);
            fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) {
                usleep(100000);
                if (++waited > 600) {
                    fprintf(stderr, "known-buffers: go-file never appeared\n");
                    return 1;
                }
            }
            dispatch_combine(queue, pso, inA, inB, heapBuf, out);
            printf("phase 2: re-ran the dispatch inside the capture\n");
        }

        printf("buffers: in_a=%luB in_b=%luB out=%luB heap_buf=%luB (heap %lu B)\n",
               (unsigned long)bytesA, (unsigned long)bytesB, (unsigned long)bytesOut,
               (unsigned long)bytesHeap, (unsigned long)hd.size);
        printf("done\n");
    }
    return 0;
}

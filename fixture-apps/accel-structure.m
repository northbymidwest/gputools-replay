// A fixture that builds one MTLAccelerationStructure, so a capture contains an
// acceleration structure for GTReplayFetchAccelerationStructure to fetch.
//
// None of the campaign's captures had an acceleration structure, so that fetch
// class (new in the 27 SDK) had never been exercised live. This builds a
// primitive acceleration structure over a single triangle - the smallest thing
// that produces a real, GPU-built MTLAccelerationStructure in the trace.
//
// Build (no Xcode project, per fixture-apps/README.md):
//   clang -fobjc-arc -fmodules -O0 -o /tmp/accel-structure \
//         fixture-apps/accel-structure.m -framework Metal -framework Foundation
//
// Capture:
//   fixture-apps/capture.sh /tmp/accel-structure captures/accel-structure.gputrace

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <unistd.h>
#include <string.h>
#include <stdlib.h>

int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            fprintf(stderr, "accel-structure: no Metal device\n");
            return 1;
        }
        if (!device.supportsRaytracing) {
            fprintf(stderr, "accel-structure: device %s has no raytracing\n",
                    device.name.UTF8String);
            return 1;
        }
        printf("device: %s (raytracing ok)\n", device.name.UTF8String);
        id<MTLCommandQueue> queue = [device newCommandQueue];

        // One triangle, tightly packed float3 positions. Env ACCEL_VERTS (nine
        // comma-separated floats) overrides the default, for decoding the
        // fetched acceleration-structure byte format by controlled variation.
        float verts[90] = {
            0.0f, 0.0f, 0.0f,
            1.0f, 0.0f, 0.0f,
            0.0f, 1.0f, 0.0f,
        };
        int nfloats = 9;
        const char *ev = getenv("ACCEL_VERTS");
        if (ev && *ev) {
            char buf[1024];
            strncpy(buf, ev, sizeof(buf) - 1);
            buf[sizeof(buf) - 1] = 0;
            int i = 0;
            for (char *tok = strtok(buf, ","); tok && i < 90; tok = strtok(NULL, ","))
                verts[i++] = strtof(tok, NULL);
            nfloats = i;
        }
        NSUInteger triCount = (NSUInteger)(nfloats / 9);
        if (triCount < 1) triCount = 1;
        printf("verts: %g,%g,%g  %g,%g,%g  %g,%g,%g\n",
               verts[0], verts[1], verts[2], verts[3], verts[4],
               verts[5], verts[6], verts[7], verts[8]);
        id<MTLBuffer> vbuf = [device newBufferWithBytes:verts
                                                 length:triCount * 9 * sizeof(float)
                                                options:MTLResourceStorageModeShared];
        vbuf.label = @"triangle_verts";

        MTLAccelerationStructureTriangleGeometryDescriptor *geo =
            [MTLAccelerationStructureTriangleGeometryDescriptor descriptor];
        geo.vertexBuffer = vbuf;
        geo.vertexStride = sizeof(float) * 3;
        geo.triangleCount = triCount;

        MTLPrimitiveAccelerationStructureDescriptor *desc =
            [MTLPrimitiveAccelerationStructureDescriptor descriptor];
        desc.geometryDescriptors = @[ geo ];
        // Refittable, so phase 2 can reference the structure with a real GPU
        // command inside the capture.
        desc.usage = MTLAccelerationStructureUsageRefit;

        MTLAccelerationStructureSizes sizes =
            [device accelerationStructureSizesWithDescriptor:desc];
        id<MTLAccelerationStructure> accel =
            [device newAccelerationStructureWithSize:sizes.accelerationStructureSize];
        accel.label = @"triangle_accel";
        id<MTLBuffer> scratch =
            [device newBufferWithLength:sizes.buildScratchBufferSize
                                options:MTLResourceStorageModePrivate];
        scratch.label = @"accel_scratch";

        id<MTLCommandBuffer> cb = [queue commandBuffer];
        id<MTLAccelerationStructureCommandEncoder> enc =
            [cb accelerationStructureCommandEncoder];
        enc.label = @"build_triangle_accel";
        [enc buildAccelerationStructure:accel
                             descriptor:desc
                           scratchBuffer:scratch
                     scratchBufferOffset:0];
        [enc endEncoding];
        [cb commit];
        [cb waitUntilCompleted];

        if (cb.error) {
            fprintf(stderr, "accel-structure: build error: %s\n",
                    cb.error.localizedDescription.UTF8String);
            return 1;
        }
        printf("phase 1: built primitive AS, %llu triangles (%llu bytes, scratch %llu)\n",
               (unsigned long long)triCount,
               (unsigned long long)sizes.accelerationStructureSize,
               (unsigned long long)sizes.buildScratchBufferSize);

        // Optional top-level (instance) acceleration structure over the BLAS,
        // gated on ACCEL_INSTANCE, to characterise GTReplayFetchAccelerationStructure
        // for an instance structure vs a primitive one.
        id<MTLAccelerationStructure> iaccel = nil;  // kept alive past the boundary
        id<MTLBuffer> instBufKeep = nil;
        if (getenv("ACCEL_INSTANCE")) {
            MTLAccelerationStructureInstanceDescriptor inst = {0};
            // Identity 4x3 transform.
            inst.transformationMatrix.columns[0].x = 1.0f;
            inst.transformationMatrix.columns[1].y = 1.0f;
            inst.transformationMatrix.columns[2].z = 1.0f;
            inst.options = MTLAccelerationStructureInstanceOptionOpaque;
            inst.mask = 0xFF;
            inst.accelerationStructureIndex = 0;
            id<MTLBuffer> instBuf =
                [device newBufferWithBytes:&inst
                                    length:sizeof(inst)
                                   options:MTLResourceStorageModeShared];
            instBuf.label = @"instance_descriptors";
            instBufKeep = instBuf;

            MTLInstanceAccelerationStructureDescriptor *idesc =
                [MTLInstanceAccelerationStructureDescriptor descriptor];
            idesc.instanceCount = 1;
            idesc.instanceDescriptorBuffer = instBuf;
            idesc.instancedAccelerationStructures = @[ accel ];

            MTLAccelerationStructureSizes isizes =
                [device accelerationStructureSizesWithDescriptor:idesc];
            iaccel =
                [device newAccelerationStructureWithSize:isizes.accelerationStructureSize];
            iaccel.label = @"triangle_instance";
            id<MTLBuffer> iscratch =
                [device newBufferWithLength:isizes.buildScratchBufferSize
                                    options:MTLResourceStorageModePrivate];
            id<MTLCommandBuffer> icb = [queue commandBuffer];
            id<MTLAccelerationStructureCommandEncoder> ienc =
                [icb accelerationStructureCommandEncoder];
            [ienc buildAccelerationStructure:iaccel
                                  descriptor:idesc
                                 scratchBuffer:iscratch
                           scratchBufferOffset:0];
            [ienc endEncoding];
            [icb commit];
            [icb waitUntilCompleted];
            if (icb.error) {
                fprintf(stderr, "accel-structure: instance build error: %s\n",
                        icb.error.localizedDescription.UTF8String);
                return 1;
            }
            printf("phase 1: built instance AS (%llu bytes)\n",
                   (unsigned long long)isizes.accelerationStructureSize);
        }

        // Two-phase (late-boundary capture): the structure above pre-exists the
        // capture boundary; phase 2 refits it with a captured command so it is
        // both snapshotted AND used inside the trace.
        const char *goFile = getenv("FIXTURE_GO_FILE");
        if (goFile && *goFile) {
            fflush(stdout);
            int waited = 0;
            while (access(goFile, F_OK) != 0) {
                usleep(100000);
                if (++waited > 600) {
                    fprintf(stderr, "accel-structure: go-file never appeared\n");
                    return 1;
                }
            }
            id<MTLCommandBuffer> cb2 = [queue commandBuffer];
            id<MTLAccelerationStructureCommandEncoder> enc2 =
                [cb2 accelerationStructureCommandEncoder];
            enc2.label = @"refit_triangle_accel";
            [enc2 refitAccelerationStructure:accel
                                  descriptor:desc
                                 destination:accel
                                scratchBuffer:scratch
                          scratchBufferOffset:0];
            [enc2 endEncoding];
            [cb2 commit];
            [cb2 waitUntilCompleted];
            if (cb2.error) {
                fprintf(stderr, "accel-structure: refit error: %s\n",
                        cb2.error.localizedDescription.UTF8String);
                return 1;
            }
            printf("phase 2: refit the acceleration structure inside the capture\n");
        }
        printf("done\n");
    }
    return 0;
}

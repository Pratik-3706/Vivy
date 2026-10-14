import React from 'react';

const FACES = ['front', 'back', 'right', 'left', 'top', 'bottom'] as const;

/**
 * Matsumoto: a small grey 3D cube with cut corners, a round recess on each
 * side and one slit eye (blue iris). An original design built from CSS faces. Idle, it
 * hovers, looks around and blinks; while a tool runs (`busy`) it spins in
 * place with its eye wide open and lit.
 */
export const MatsumotoCube: React.FC<{ size?: number; busy?: boolean }> = ({ size = 14, busy = false }) => (
  <span
    className={`m3d ${busy ? 'busy' : 'idle'}`}
    style={{ '--s': `${size}px` } as React.CSSProperties}
    aria-hidden="true"
  >
    <span className="m3d-hover">
      <span className="m3d-cube">
        <i className="m3d-face front">
          <b className="m3d-ring">
            <b className="m3d-lens">
              <b className="m3d-iris" />
              <b className="m3d-lid upper" />
              <b className="m3d-lid lower" />
            </b>
          </b>
        </i>
        {FACES.slice(1).map((f) => (
          <i key={f} className={`m3d-face ${f}`} />
        ))}
      </span>
    </span>
  </span>
);

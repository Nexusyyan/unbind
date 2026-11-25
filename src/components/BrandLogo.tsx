interface BrandLogoProps {
  className?: string;
}

export function BrandLogo({ className = "h-[17px] w-[17px]" }: BrandLogoProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
      className={className}
    >
      {/* Decouple emblem: twin architectural U-links unbinding along a -45deg axis */}
      <g
        transform="rotate(-45 12 12)"
        stroke="currentColor"
        strokeWidth="2.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M 10.6 10.65 H 7.5 A 2.55 2.55 0 0 0 7.5 15.75 H 10.6" />
        <path d="M 13.4 8.25 H 16.5 A 2.55 2.55 0 0 1 16.5 13.35 H 13.4" opacity="0.52" />
      </g>
    </svg>
  );
}

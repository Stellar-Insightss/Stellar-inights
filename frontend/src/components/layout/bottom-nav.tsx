"use client";

import React from "react";
import { Link } from "@/i18n/navigation";
import { usePathname } from "@/i18n/navigation";
import { LayoutDashboard, Users, Wallet, Code2, Medal } from "lucide-react";

interface NavItem {
  name: string;
  href: string;
  icon: React.ReactNode;
  id: string;
}

// Mirrors the sidebar's grouped IA (see layout/sidebar.tsx). Bottom-nav space
// only fits a handful of tabs, so this surfaces one representative route per
// top-priority group — Overview, Assets, Wallets, Soroban, Rankings — and
// drops Validators/Explorer, matching the same groups sidebar.tsx demotes
// into its lower-priority "explorer" section.
const navItems: NavItem[] = [
  {
    name: "Overview",
    href: "/dashboard",
    icon: <LayoutDashboard className="w-5 h-5" />,
    id: "overview",
  },
  {
    name: "Assets",
    href: "/trustlines",
    icon: <Users className="w-5 h-5" />,
    id: "assets",
  },
  {
    name: "Wallets",
    href: "/wallet",
    icon: <Wallet className="w-5 h-5" />,
    id: "wallets",
  },
  {
    name: "Soroban",
    href: "/soroban",
    icon: <Code2 className="w-5 h-5" />,
    id: "soroban",
  },
  {
    name: "Rankings",
    href: "/rankings",
    icon: <Medal className="w-5 h-5" />,
    id: "rankings",
  },
];

export function BottomNav() {
  const pathname = usePathname();

  const isActive = (href: string) => {
    return pathname === href || pathname.startsWith(href + "/");
  };

  return (
    <nav className="fixed bottom-0 left-0 right-0 bg-white dark:bg-slate-900 border-t border-gray-200 dark:border-slate-700 lg:hidden z-50 safe-area-inset-bottom">
      <div className="flex items-center justify-around h-16 px-2">
        {navItems.map((item) => {
          const active = isActive(item.href);
          return (
            <Link
              key={item.id}
              href={item.href}
              className={`flex flex-col items-center justify-center gap-1 px-3 py-2 rounded-lg transition-colors min-w-[44px] min-h-[44px] touch-manipulation ${
                active
                  ? "text-blue-500"
                  : "text-muted-foreground dark:text-muted-foreground active:bg-gray-100 dark:active:bg-slate-800"
              }`}
              aria-current={active ? "page" : undefined}
              aria-label={item.name}
            >
              <div className={active ? "scale-110" : ""}>{item.icon}</div>
              <span className="text-xs font-medium">{item.name}</span>
            </Link>
          );
        })}
      </div>
    </nav>
  );
}
